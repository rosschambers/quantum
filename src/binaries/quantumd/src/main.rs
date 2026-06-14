mod gtk_loop;
mod plugin_loop;
mod runtime;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::value::RawValue;
use serde_json::Value;
use tracing::info;
use tracing_subscriber::EnvFilter;

use quantum_application::{
    ApplicationError, Dispatcher as AppDispatcher, LaunchActionUseCase, ListProvidersUseCase,
    OpenViewUseCase, QueryProviderUseCase, ReloadPluginsUseCase, ReloadThemeUseCase,
    ScheduleActionUseCase, SearchUseCase, SetThemeUseCase, SubscribeProviderUseCase,
};
use quantum_config::{Config, ConfigStore};
use quantum_domain::{DomainError, EventBus, ProviderId, ProviderSource};
use quantum_hyprland::HyprlandSocketClient;
use quantum_ipc::{
    DispatchError, DispatchResult, Dispatcher as IpcDispatcher, EventEnvelope, UnixSocketServer,
};
use quantum_providers::{
    BluezProvider, DeclarativeShellProvider, DesktopAppsProvider, HyprlandActiveWindowProvider,
    HyprlandWindowsProvider, InMemoryProviderRegistry, LogindBrightnessProvider, MprisProvider,
    NetworkManagerProvider, PluginScriptProvider, PowerProfilesDaemonProvider, ProcStatsProvider,
    ProvidersError, PulseAudioProvider, ShellCommandProvider, SystemPowerProvider,
    TokioShellExecutor, UpowerBatteryProvider, WifiProvider,
};
use quantum_theme::ThemeStore;
use quantum_ui::{DummyWindowHost, IpcDispatcher as UiIpcDispatcher};

/// Adapter that lets the infrastructure IPC server route requests into the
/// application-layer dispatcher. The adapter converts `ApplicationError` into
/// the wire-level `DispatchError` so Rust types never leak across IPC.
struct AppDispatcherAdapter {
    inner: Arc<AppDispatcher>,
}

impl AppDispatcherAdapter {
    fn new(inner: Arc<AppDispatcher>) -> Self {
        Self { inner }
    }
}

/// Lower an `ApplicationError` into the `(code, message)` pair that every
/// wire-level `DispatchError` carries. Both the infrastructure `IpcDispatcher`
/// and the UI `IpcDispatcher` need the same mapping, but their
/// `DispatchError` structs live in different crates (the ui crate cannot
/// depend on infrastructure per the onion-architecture rule) so we map to
/// the common shape here and each impl block wraps the tuple in its own
/// error type.
fn application_error_parts(err: &ApplicationError) -> (i32, String) {
    (err.rpc_code(), err.to_string())
}

#[async_trait]
impl IpcDispatcher for AppDispatcherAdapter {
    async fn dispatch(&self, method: &str, params: Option<&RawValue>) -> DispatchResult {
        match self.inner.dispatch(method, params).await {
            Ok(value) => Ok(value),
            Err(err) => {
                let (code, message) = application_error_parts(&err);
                Err(DispatchError::new(code, message))
            }
        }
    }
}

#[async_trait]
impl UiIpcDispatcher for AppDispatcherAdapter {
    async fn dispatch(
        &self,
        method: &str,
        params: Value,
    ) -> quantum_ui::dispatcher::DispatchResult {
        // The UI bridge still hands us a fully-parsed `Value` because the
        // ui crate cannot depend on serde_json's `raw_value` shape across
        // its public trait. Bounce through `to_raw_value` so the
        // application dispatcher sees the unified `Option<&RawValue>`
        // contract. A `Value::Null` here means the JS bridge omitted
        // params; forward `None` so handlers can distinguish "no params"
        // from "empty object".
        let raw = if params.is_null() {
            None
        } else {
            match serde_json::value::to_raw_value(&params) {
                Ok(r) => Some(r),
                Err(err) => {
                    return Err(quantum_ui::dispatcher::DispatchError {
                        code: -32603,
                        message: format!("failed to encode UI params: {err}"),
                    });
                }
            }
        };
        match self.inner.dispatch(method, raw.as_deref()).await {
            Ok(value) => Ok(value),
            Err(err) => {
                let (code, message) = application_error_parts(&err);
                Err(quantum_ui::dispatcher::DispatchError { code, message })
            }
        }
    }
}

/// An EventBus adapter that broadcasts domain events to IPC clients.
struct BroadcastingEventBus {
    tx: tokio::sync::broadcast::Sender<EventEnvelope>,
}

impl BroadcastingEventBus {
    fn new(tx: tokio::sync::broadcast::Sender<EventEnvelope>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl EventBus for BroadcastingEventBus {
    async fn publish(&self, event: &str, payload: &str) -> Result<(), DomainError> {
        // Forward the raw payload string into a `RawValue` instead of parsing
        // to `serde_json::Value`. Every downstream consumer (IPC server,
        // panel + widget WebView bridges) ultimately re-serializes the
        // payload to JSON text anyway; round-tripping through `Value` was
        // pure waste. Falling back to JSON `null` keeps the contract that
        // every publish produces a well-formed envelope even if a provider
        // pushes garbage.
        let raw =
            serde_json::value::RawValue::from_string(payload.to_string()).unwrap_or_else(|_| {
                serde_json::value::RawValue::from_string("null".to_string())
                    .expect("\"null\" is valid JSON")
            });
        let _ = self.tx.send(EventEnvelope {
            channel: event.to_string(),
            payload: raw,
        });
        Ok(())
    }
}

/// First-party plugins compiled into the daemon. The build script
/// (`build.rs`) stages each plugin's `views/<view>/view.toml` and built
/// `dist/` output into `$OUT_DIR/embedded-plugins`, so this static never
/// contains pnpm `node_modules`, Svelte sources, or tooling configs.
///
/// Constraint: the view `dist/` directories must exist when quantumd is
/// compiled. Build them first with
/// `pnpm -C src/ui/plugins/<name>/views/<name> build`; views without a
/// `dist/` are skipped by the build script with a cargo warning.
static EMBEDDED_PLUGINS: include_dir::Dir<'static> =
    include_dir::include_dir!("$OUT_DIR/embedded-plugins");

/// Resolve the user-side plugins directory. Falls back to `~/.config`
/// if `XDG_CONFIG_HOME` is unset, matching the convention used by the
/// theme store and standard XDG semantics.
fn plugins_dir() -> PathBuf {
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(config_home).join("quantum/plugins")
}

/// Walk the user plugins directory and merge the result over the
/// embedded first-party catalog (user plugins shadow embedded ones by
/// name). Returns the merged list plus the separate embedded and user
/// counts for logging. A failed user walk degrades to "no user plugins"
/// with a warning; the embedded catalog is compiled in and cannot fail.
fn discover_merged_plugins(
    user_plugins_dir: &std::path::Path,
    embedded: &include_dir::Dir<'static>,
) -> (Vec<quantum_plugins::PluginDescription>, usize, usize) {
    let user_plugins = match quantum_plugins::walk(user_plugins_dir) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(
                "failed to walk plugins directory {}: {e}; continuing with embedded plugins only",
                user_plugins_dir.display()
            );
            Vec::new()
        }
    };
    let embedded_plugins = match quantum_plugins::walk_embedded(embedded) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("failed to walk embedded plugins: {e}; continuing without them");
            Vec::new()
        }
    };
    let embedded_count = embedded_plugins.len();
    let user_count = user_plugins.len();
    let merged = quantum_plugins::merge_plugins(user_plugins, embedded_plugins);
    (merged, embedded_count, user_count)
}

/// Domain `PluginCatalog` implementation backed by
/// `quantum_plugins::walk` merged over the embedded first-party catalog,
/// mirroring startup discovery exactly. Used by the `plugin.reload` IPC
/// handler, so reload counts embedded plugins too.
struct FilesystemPluginCatalog {
    plugins_dir: PathBuf,
    embedded: &'static include_dir::Dir<'static>,
}

#[async_trait::async_trait]
impl quantum_domain::PluginCatalog for FilesystemPluginCatalog {
    async fn discover(&self) -> std::result::Result<usize, quantum_domain::DomainError> {
        let plugins_dir = self.plugins_dir.clone();
        let embedded = self.embedded;
        // Walk the filesystem on a blocking pool so we never park the
        // async runtime on disk I/O.
        let (merged, _, _) =
            tokio::task::spawn_blocking(move || discover_merged_plugins(&plugins_dir, embedded))
                .await
                .map_err(|e| {
                    quantum_domain::DomainError::Unsupported(format!("plugin walk join error: {e}"))
                })?;
        Ok(merged.len())
    }
}

/// Register a fallible provider into the in-memory registry, or warn and
/// continue if its connect failed. Collapses the previously-repeated
/// `match ::connect()` boilerplate now that the tray providers are
/// constructed in parallel via `tokio::join!`.
async fn register_or_warn<P: quantum_domain::ProviderSource + 'static>(
    registry: &InMemoryProviderRegistry,
    name: &str,
    result: Result<P, ProvidersError>,
) {
    match result {
        Ok(provider) => {
            let provider: Arc<dyn quantum_domain::ProviderSource> = Arc::new(provider);
            registry
                .register(provider.id().clone(), provider.clone())
                .await;
            info!("Registered {name}");
        }
        Err(err) => tracing::warn!(error = ?err, "{name} unavailable"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("Starting quantumd v{}", env!("CARGO_PKG_VERSION"));

    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|a| a == "--headless");
    let socket_override = parse_socket_override(&args);

    // 4 workers cover IPC accept + per-connection handlers + provider tasks;
    // the default `num_cpus()` is wasteful for a daemon mostly waiting on
    // sockets and timers, and on machines with many cores it burns idle
    // wakeups for no throughput benefit.
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .thread_name("quantum-worker")
        .build()?;
    let worker = runtime::spawn_worker(tokio_runtime)?;

    // Set up window host (GTK or dummy).
    //
    // For GTK we also grab a clone of the underlying sender via
    // `GtkWindowHost::sender()` before the host is type-erased into an
    // `Arc<dyn WindowHost>`. The GTK loop needs it to install the
    // `BarMultiplexer`, which pushes `WindowRequest`s on the same channel
    // that `GtkWindowHost::open` uses. In headless mode we keep the sender
    // around so the tuple shape stays uniform; it is never used because the
    // GTK loop never runs.
    let (window_host, window_rx, window_request_tx) = if headless {
        let dummy = Arc::new(DummyWindowHost::new()) as Arc<dyn quantum_domain::ports::WindowHost>;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<quantum_ui::WindowRequest>();
        (dummy, rx, tx)
    } else {
        let (host, rx) = quantum_ui::GtkWindowHost::new();
        let tx = host.sender();
        (
            Arc::new(host) as Arc<dyn quantum_domain::ports::WindowHost>,
            rx,
            tx,
        )
    };

    // All async setup happens on the worker.
    let setup = worker
        .handle
        .block_on(async { setup_daemon(socket_override, window_host).await })?;

    // Start watching theme files for changes and emit events
    setup
        .theme_store_concrete
        .clone()
        .start_watching(setup.event_bus.clone());

    if headless {
        // Run signal loop on the worker, blocking the main thread.
        worker.handle.block_on(async move {
            run_signal_loop(setup.socket_path).await;
        });
    } else {
        let app = gtk4::Application::builder()
            .application_id("dev.quantum.daemon")
            .build();

        // Compute the per-monitor multiplexed view list from descriptors:
        // every catalog entry whose descriptor declares both `per_monitor`
        // and `auto_show`. These are spawned per-monitor on the GTK thread by
        // the `ViewMultiplexer` (see `gtk_loop::run`'s `multiplexed_views`
        // path), because monitor enumeration requires `gdk::Display::default()`
        // which is GTK-thread-only.
        //
        // A user can opt a view out of multiplexing by adding a `[[widget]]`
        // entry to config.toml naming the view (by its canonical
        // `plugin/<plugin>/<view>` name OR its legacy alias, for example
        // `widgets/bar`) with `auto_show = false`. We canonicalize both the
        // descriptor names and the config-override names before comparing so
        // a legacy alias in config disables the matching canonical view.
        let disabled_views: std::collections::HashSet<String> = setup
            .config
            .widget
            .iter()
            .filter(|w| !w.auto_show)
            .map(|w| quantum_ui::canonicalize_view_name(&w.view))
            .collect();
        let multiplexed_views: Vec<String> = setup
            .view_catalog_entries
            .iter()
            .filter(|(_, descriptor)| descriptor.per_monitor && descriptor.auto_show)
            .map(|(name, _)| name.clone())
            .filter(|name| !disabled_views.contains(name))
            .collect();
        if multiplexed_views.is_empty() {
            tracing::info!("ViewMultiplexer install skipped: no per-monitor auto-show views");
        } else {
            tracing::info!(
                "ViewMultiplexer install enabled for per-monitor views: {:?}",
                multiplexed_views
            );
        }

        // Spawn a task to auto-show non-multiplexed widgets after a brief
        // delay for GTK to activate. Per-monitor multiplexed views are
        // excluded here: they are handled by the `ViewMultiplexer` above.
        let dispatcher_for_autoshow = setup.ipc_dispatcher.clone();
        let multiplexed_canonical: std::collections::HashSet<String> =
            multiplexed_views.iter().cloned().collect();
        let widgets_to_show: Vec<String> = setup
            .config
            .widget
            .iter()
            .filter(|w| {
                w.auto_show
                    && !multiplexed_canonical.contains(&quantum_ui::canonicalize_view_name(&w.view))
            })
            .map(|w| w.view.clone())
            .collect();
        if !widgets_to_show.is_empty() {
            worker.handle.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                for view in widgets_to_show {
                    let params = serde_json::json!({"name": view});
                    if let Err(err) = dispatcher_for_autoshow.dispatch("view.show", params).await {
                        tracing::warn!("auto-show widget {view} failed: {:?}", err);
                    }
                }
            });
        }

        let view_catalog = quantum_ui::ViewCatalog::from_plugins(setup.view_catalog_entries);
        let _exit_code = crate::gtk_loop::run(
            &app,
            window_rx,
            setup.ipc_dispatcher,
            setup.theme_store,
            worker.handle.clone(),
            setup.event_tx.clone(),
            window_request_tx,
            multiplexed_views,
            view_catalog,
        );
        // After GTK exits, clean up socket.
        let _ = std::fs::remove_file(&setup.socket_path);
        return Ok(());
    }

    worker.shutdown();
    Ok(())
}

struct DaemonSetup {
    socket_path: std::path::PathBuf,
    ipc_dispatcher: Arc<dyn UiIpcDispatcher>,
    theme_store: Arc<dyn quantum_domain::ports::ThemeStore>,
    theme_store_concrete: Arc<ThemeStore>,
    event_bus: Arc<dyn quantum_domain::EventBus>,
    event_tx: tokio::sync::broadcast::Sender<EventEnvelope>,
    config: Config,
    view_catalog_entries: Vec<(String, quantum_domain::ViewDescriptor)>,
}

async fn setup_daemon(
    socket_override: Option<String>,
    window_host: Arc<dyn quantum_domain::ports::WindowHost>,
) -> Result<DaemonSetup, Box<dyn std::error::Error>> {
    // Load configuration. A missing config file is not fatal: ConfigStore::load
    // already falls back to defaults.
    let config_store = match ConfigStore::load().await {
        Ok(store) => store,
        Err(err) => {
            return Err(format!("failed to load configuration: {err}").into());
        }
    };
    let config = config_store.get_config().await;

    let active_theme = config.general.active_theme.clone();

    let theme_store =
        Arc::new(ThemeStore::new(active_theme).with_embedded_plugins(&EMBEDDED_PLUGINS));
    let shell_executor = Arc::new(TokioShellExecutor::new());
    let registry = Arc::new(InMemoryProviderRegistry::new());

    // Create broadcast channel for event notifications to IPC clients.
    let (event_tx, _) = tokio::sync::broadcast::channel::<EventEnvelope>(64);
    let event_bus: Arc<dyn quantum_domain::EventBus> =
        Arc::new(BroadcastingEventBus::new(event_tx.clone()));

    // Desktop apps provider
    match DesktopAppsProvider::new(shell_executor.clone()).await {
        Ok(provider) => {
            let id = provider.id().clone();
            registry
                .register(
                    id,
                    Arc::new(provider) as Arc<dyn quantum_domain::ProviderSource>,
                )
                .await;
            info!("Registered DesktopAppsProvider");
        }
        Err(err) => {
            tracing::warn!("Failed to create DesktopAppsProvider: {err}");
        }
    }

    // Shell command provider
    let shell_cmd = Arc::new(ShellCommandProvider::new(shell_executor.clone()));
    let shell_cmd_id = shell_cmd.id().clone();
    registry
        .register(
            shell_cmd_id,
            shell_cmd as Arc<dyn quantum_domain::ProviderSource>,
        )
        .await;
    info!("Registered ShellCommandProvider");

    // Hyprland provider (optional)
    let mut hypr_client_opt: Option<Arc<HyprlandSocketClient>> = None;
    match HyprlandSocketClient::new() {
        Ok(client) => {
            let client_arc = Arc::new(client);
            hypr_client_opt = Some(client_arc.clone());
            match HyprlandWindowsProvider::new(client_arc).await {
                Ok(provider) => {
                    let id = provider.id().clone();
                    registry
                        .register(
                            id,
                            Arc::new(provider) as Arc<dyn quantum_domain::ProviderSource>,
                        )
                        .await;
                    info!("Registered HyprlandWindowsProvider");
                }
                Err(err) => {
                    tracing::warn!("Failed to create HyprlandWindowsProvider: {err}");
                }
            }
        }
        Err(_) => {
            tracing::warn!("Hyprland not available. Continuing without Hyprland support.");
        }
    }

    // Register declarative shell providers from config
    for provider_config in &config.provider {
        match DeclarativeShellProvider::new(provider_config.clone(), shell_executor.clone()) {
            Ok(provider) => {
                let id = ProviderId::from(provider_config.id.clone());
                info!(
                    "Registered DeclarativeShellProvider: {}",
                    provider_config.id
                );
                registry
                    .register(
                        id,
                        Arc::new(provider) as Arc<dyn quantum_domain::ProviderSource>,
                    )
                    .await;
            }
            Err(err) => {
                tracing::warn!("Failed to register {}: {}", provider_config.id, err);
            }
        }
    }

    // ProcStatsProvider — never fails; /proc is universally available on Linux.
    let proc_stats = Arc::new(ProcStatsProvider::new(tokio::runtime::Handle::current()));
    registry
        .register(
            proc_stats.id().clone(),
            proc_stats.clone() as Arc<dyn quantum_domain::ProviderSource>,
        )
        .await;
    info!("Registered ProcStatsProvider");

    // MprisProvider — provider is registered even if DBus fails (the inner task degrades to publishing 'no player' state).
    let mpris = Arc::new(MprisProvider::new(tokio::runtime::Handle::current()));
    registry
        .register(
            mpris.id().clone(),
            mpris.clone() as Arc<dyn quantum_domain::ProviderSource>,
        )
        .await;
    info!("Registered MprisProvider");

    // HyprlandActiveWindowProvider — gated on the same conditional that already gates HyprlandWindowsProvider.
    if let Some(hypr_client) = &hypr_client_opt {
        let active_window = Arc::new(HyprlandActiveWindowProvider::new(
            hypr_client.clone(),
            tokio::runtime::Handle::current(),
        ));
        registry
            .register(
                active_window.id().clone(),
                active_window as Arc<dyn quantum_domain::ProviderSource>,
            )
            .await;
        info!("Registered HyprlandActiveWindowProvider");
    }

    // Tray providers and the action-only system_power provider — each
    // registered with graceful fallback. If the underlying service is
    // missing the provider still publishes an unavailable state so the
    // frontend has a uniform contract.
    //
    // All seven of these connects are DBus or pactl service-discovery
    // round-trips with no dependency on each other, so we fan them out
    // with `tokio::join!` and then register the results sequentially.
    // Serially awaiting each connect added several seconds of cold-start
    // latency on systems where every service responds.
    let lock_command_cfg = config
        .system_power
        .as_ref()
        .and_then(|sp| sp.lock_command.clone());
    let runtime_handle = tokio::runtime::Handle::current();
    let (battery, network, bluez, ppd, brightness, audio, sysp, wifi) = tokio::join!(
        UpowerBatteryProvider::connect(),
        NetworkManagerProvider::connect(),
        BluezProvider::connect(),
        PowerProfilesDaemonProvider::connect(),
        LogindBrightnessProvider::connect(runtime_handle.clone()),
        PulseAudioProvider::connect(runtime_handle.clone()),
        SystemPowerProvider::connect(lock_command_cfg),
        WifiProvider::connect(),
    );
    register_or_warn(&registry, "UpowerBatteryProvider", battery).await;
    register_or_warn(&registry, "NetworkManagerProvider", network).await;
    register_or_warn(&registry, "BluezProvider", bluez).await;
    register_or_warn(&registry, "PowerProfilesDaemonProvider", ppd).await;
    register_or_warn(&registry, "LogindBrightnessProvider", brightness).await;
    register_or_warn(&registry, "PulseAudioProvider", audio).await;
    register_or_warn(&registry, "SystemPowerProvider", sysp).await;
    register_or_warn(&registry, "WifiProvider", wifi).await;

    // Plugins: walk ~/.config/quantum/plugins/, merge over the embedded
    // first-party catalog (user plugins shadow embedded ones by name),
    // register one PluginScriptProvider per discovered plugin, spawn one
    // polling task per polled script. Failure to walk (or to register any
    // individual plugin) logs a warning but never aborts startup — the
    // daemon must come up with built-in providers regardless of plugin
    // state.
    let plugins_directory = plugins_dir();
    let (plugin_descs, embedded_plugin_count, user_plugin_count) =
        discover_merged_plugins(&plugins_directory, &EMBEDDED_PLUGINS);
    info!(
        "plugins discovered: {embedded_plugin_count} embedded, {user_plugin_count} user, {} after merge",
        plugin_descs.len()
    );
    // Flatten plugin views into (canonical name, descriptor) tuples for the
    // window registry's ViewCatalog. quantum-ui cannot depend on the plugin
    // discovery crate, so the daemon does the flattening here, from the same
    // merged list used for provider registration below.
    let view_catalog_entries: Vec<(String, quantum_domain::ViewDescriptor)> = plugin_descs
        .iter()
        .flat_map(|plugin| {
            plugin.views.iter().map(|view| {
                (
                    format!("plugin/{}/{}", plugin.name, view.name),
                    view.descriptor.clone(),
                )
            })
        })
        .collect();
    let mut total_plugins = 0usize;
    let mut total_polled = 0usize;
    let mut total_idle = 0usize;
    let mut total_actions = 0usize;
    let mut total_views = 0usize;
    for desc in plugin_descs {
        total_plugins += 1;
        total_polled += desc.polled_scripts.len();
        total_idle += desc.idle_scripts.len();
        total_actions += desc.actions.len();
        total_views += desc.views.len();

        for ps in &desc.polled_scripts {
            let channel = ps.channel.clone();
            let interval = ps.interval;
            let command = ps.command.clone();
            let plugin_dir = desc.dir.clone();
            let plugin_name = desc.name.clone();
            let event_bus_clone = event_bus.clone();
            tokio::spawn(async move {
                plugin_loop::run_polling_script_loop(
                    channel,
                    interval,
                    command,
                    plugin_dir,
                    plugin_name,
                    event_bus_clone,
                )
                .await;
            });
        }

        let provider = Arc::new(PluginScriptProvider::new(
            &desc.name,
            desc.polled_scripts.clone(),
            desc.idle_scripts.clone(),
            desc.actions.clone(),
            shell_executor.clone(),
        ));
        registry
            .register(
                provider.id().clone(),
                provider as Arc<dyn quantum_domain::ProviderSource>,
            )
            .await;
        info!("Registered plugin '{}'", desc.name);
    }
    info!(
        "Loaded {} plugins ({} polled scripts, {} idle scripts, {} actions, {} views)",
        total_plugins, total_polled, total_idle, total_actions, total_views
    );

    // Use cases
    let search_use_case = Arc::new(SearchUseCase::new(registry.clone()));
    let launch_action_use_case = Arc::new(LaunchActionUseCase::new(registry.clone()));
    let list_providers_use_case = Arc::new(ListProvidersUseCase::new(registry.clone()));
    let reload_theme_use_case = Arc::new(ReloadThemeUseCase::new(
        theme_store.clone() as Arc<dyn quantum_domain::ThemeStore>,
        event_bus.clone(),
    ));
    let set_theme_use_case = Arc::new(SetThemeUseCase::new(
        theme_store.clone() as Arc<dyn quantum_domain::ThemeStore>,
        event_bus.clone(),
    ));
    let subscribe_provider_use_case = Arc::new(SubscribeProviderUseCase::new(
        registry.clone(),
        event_bus.clone(),
    ));
    let query_provider_use_case = Arc::new(QueryProviderUseCase::new(registry.clone()));

    // Pre-subscribe to the system providers so streams start publishing immediately
    let _ = subscribe_provider_use_case
        .execute("system.stats".into())
        .await;
    let _ = subscribe_provider_use_case.execute("mpris".into()).await;
    if hypr_client_opt.is_some() {
        let _ = subscribe_provider_use_case
            .execute("hyprland.activewindow".into())
            .await;
    }
    // Pre-subscribe tray providers (silent fail if not registered).
    for id in [
        "power",
        "network",
        "bluetooth",
        "power_profile",
        "brightness",
        "audio",
        "system_power",
        "wifi",
    ] {
        let _ = subscribe_provider_use_case.execute(id.into()).await;
    }

    // Use the window host passed in from main (GtkWindowHost when running with
    // GTK, DummyWindowHost when headless).
    let open_view_use_case = Arc::new(OpenViewUseCase::new(window_host));

    let schedule_action_use_case =
        Arc::new(ScheduleActionUseCase::new(launch_action_use_case.clone()));
    let plugins_directory = plugins_dir();
    let plugin_catalog: Arc<dyn quantum_domain::PluginCatalog> =
        Arc::new(FilesystemPluginCatalog {
            plugins_dir: plugins_directory,
            embedded: &EMBEDDED_PLUGINS,
        });
    let reload_plugins_use_case = Arc::new(ReloadPluginsUseCase::new(plugin_catalog));
    let dispatcher = Arc::new(AppDispatcher::new(
        search_use_case,
        launch_action_use_case,
        list_providers_use_case,
        reload_theme_use_case,
        set_theme_use_case,
        open_view_use_case,
        subscribe_provider_use_case,
        query_provider_use_case,
        schedule_action_use_case,
        reload_plugins_use_case,
    ));
    let _ipc_dispatcher = Arc::new(AppDispatcherAdapter::new(dispatcher));

    // Determine socket path
    let socket_path = if let Some(path) = socket_override {
        PathBuf::from(path)
    } else {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("{}/.run", std::env::var("HOME").unwrap_or_default()));
        PathBuf::from(runtime_dir).join("quantum.sock")
    };

    // Clean up stale socket if needed
    if socket_path.exists() {
        if tokio::net::UnixStream::connect(&socket_path).await.is_err() {
            if let Err(err) = std::fs::remove_file(&socket_path) {
                tracing::warn!("Failed to remove stale socket: {err}");
            }
        } else {
            eprintln!("quantum is already running");
            std::process::exit(1);
        }
    }

    // Start IPC server
    let server = Arc::new(UnixSocketServer::new(&socket_path));
    {
        let server = server.clone();
        let dispatcher = _ipc_dispatcher.clone();
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(err) = server.serve(dispatcher, event_tx).await {
                tracing::error!("IPC server error: {err}");
            }
        });
    }

    info!("IPC server listening on {}", socket_path.display());

    Ok(DaemonSetup {
        socket_path,
        ipc_dispatcher: _ipc_dispatcher,
        theme_store: theme_store.clone() as Arc<dyn quantum_domain::ports::ThemeStore>,
        theme_store_concrete: theme_store,
        event_bus,
        event_tx,
        config,
        view_catalog_entries,
    })
}

async fn run_signal_loop(socket_path: std::path::PathBuf) {
    let socket_path_clone = socket_path.clone();
    let sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
    let sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt());

    info!("Running daemon (headless mode)");

    match (sigterm, sigint) {
        (Ok(mut sigterm), Ok(mut sigint)) => {
            tokio::select! {
                _ = sigterm.recv() => info!("Received SIGTERM"),
                _ = sigint.recv() => info!("Received SIGINT"),
                _ = tokio::signal::ctrl_c() => info!("Received Ctrl+C"),
            }
        }
        (sigterm_res, sigint_res) => {
            if let Err(err) = sigterm_res {
                tracing::warn!("failed to install SIGTERM handler: {err}");
            }
            if let Err(err) = sigint_res {
                tracing::warn!("failed to install SIGINT handler: {err}");
            }
            // Fall back to ctrl_c only.
            let _ = tokio::signal::ctrl_c().await;
            info!("Received Ctrl+C");
        }
    }

    if let Err(err) = std::fs::remove_file(&socket_path_clone) {
        tracing::warn!("Failed to remove socket file: {err}");
    }
}

fn parse_socket_override(args: &[String]) -> Option<String> {
    if let Some(value) = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--socket=").map(|s| s.to_string()))
    {
        return Some(value);
    }

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--socket" {
            if let Some(next) = iter.next() {
                return Some(next.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip a payload through `BroadcastingEventBus::publish` and make
    /// sure subscribers receive JSON that parses back to the original value.
    /// This is the public contract we care about: structure preserved,
    /// regardless of whether the bus parses to `Value` or forwards raw text.
    #[tokio::test]
    async fn publish_preserves_payload_structure_for_subscribers() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<EventEnvelope>(16);
        let bus = BroadcastingEventBus::new(tx);

        let original = serde_json::json!({"a": 1, "nested": {"b": [1, 2, 3]}});
        let payload_str = original.to_string();
        bus.publish("test.channel", &payload_str)
            .await
            .expect("publish");

        let env = rx.recv().await.expect("subscriber receives envelope");
        assert_eq!(env.channel, "test.channel");

        let parsed: Value = serde_json::from_str(env.payload.get()).expect("payload is valid JSON");
        assert_eq!(parsed, original);
    }

    #[tokio::test]
    async fn publish_with_invalid_json_falls_back_to_null() {
        // Garbage in -> the bus must still emit a valid envelope so the
        // broadcast channel doesn't accumulate half-formed messages.
        let (tx, mut rx) = tokio::sync::broadcast::channel::<EventEnvelope>(16);
        let bus = BroadcastingEventBus::new(tx);

        bus.publish("garbage.channel", "this is not json")
            .await
            .expect("publish");

        let env = rx.recv().await.expect("subscriber receives envelope");
        assert_eq!(env.channel, "garbage.channel");
        assert_eq!(env.payload.get(), "null");
    }

    /// Test-only embedded catalog: `alpha` and `beta`, each with one view.
    static TEST_EMBEDDED_PLUGINS: include_dir::Dir<'static> =
        include_dir::include_dir!("$CARGO_MANIFEST_DIR/test-fixtures/embedded-plugins");

    /// `plugin.reload` must see the same merged catalog as startup:
    /// embedded plugins count, and a user plugin with the same name as
    /// an embedded one shadows it instead of double-counting.
    #[tokio::test]
    async fn catalog_discover_merges_embedded_and_user_plugins() {
        let user_dir = tempfile::tempdir().expect("tempdir");
        // `alpha` shadows the embedded plugin of the same name; `zeta`
        // exists only on the user side.
        std::fs::create_dir_all(user_dir.path().join("alpha")).expect("mkdir alpha");
        std::fs::create_dir_all(user_dir.path().join("zeta")).expect("mkdir zeta");

        let catalog = FilesystemPluginCatalog {
            plugins_dir: user_dir.path().to_path_buf(),
            embedded: &TEST_EMBEDDED_PLUGINS,
        };
        let count = quantum_domain::PluginCatalog::discover(&catalog)
            .await
            .expect("discover");

        // embedded {alpha, beta} merged with user {alpha, zeta}
        // -> {alpha (user), beta, zeta}
        assert_eq!(count, 3);
    }

    /// With no user plugins directory at all, discover still reports the
    /// embedded plugins.
    #[tokio::test]
    async fn catalog_discover_reports_embedded_when_user_directory_is_missing() {
        let catalog = FilesystemPluginCatalog {
            plugins_dir: PathBuf::from("/nonexistent/quantum/plugins"),
            embedded: &TEST_EMBEDDED_PLUGINS,
        };
        let count = quantum_domain::PluginCatalog::discover(&catalog)
            .await
            .expect("discover");
        assert_eq!(count, 2);
    }
}
