mod gtk_loop;
mod runtime;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tracing::info;
use tracing_subscriber::EnvFilter;

use quantum_application::{
    Dispatcher as AppDispatcher, LaunchActionUseCase, ListProvidersUseCase, OpenViewUseCase,
    QueryProviderUseCase, ReloadThemeUseCase, ScheduleActionUseCase, SearchUseCase,
    SubscribeProviderUseCase,
};
use quantum_config::{Config, ConfigStore};
use quantum_domain::{DomainError, EventBus, ProviderId, ProviderSource};
use quantum_infrastructure::{
    providers::DesktopAppsProvider,
    providers::{
        BluezProvider, LogindBrightnessProvider, NetworkManagerProvider,
        PowerProfilesDaemonProvider, PulseAudioProvider, SystemPowerProvider,
        UpowerBatteryProvider,
    },
    registry::InMemoryProviderRegistry,
    shell::TokioShellExecutor,
    HyprlandActiveWindowProvider, HyprlandSocketClient, MprisProvider, ProcStatsProvider,
    ShellCommandProvider,
};
use quantum_ipc::{
    DispatchError, DispatchResult, Dispatcher as IpcDispatcher, EventEnvelope, UnixSocketServer,
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

#[async_trait]
impl IpcDispatcher for AppDispatcherAdapter {
    async fn dispatch(&self, method: &str, params: Value) -> DispatchResult {
        match self.inner.dispatch(method, params).await {
            Ok(value) => Ok(value),
            Err(err) => Err(DispatchError::new(err.rpc_code(), err.to_string())),
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
        match self.inner.dispatch(method, params).await {
            Ok(value) => Ok(value),
            Err(err) => Err(quantum_ui::dispatcher::DispatchError {
                code: err.rpc_code(),
                message: err.to_string(),
            }),
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

    async fn subscribe(&self, _event: &str) -> Result<(), DomainError> {
        Ok(())
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

        // Spawn a task to auto-show widgets after a brief delay for GTK to activate.
        // `widgets/bar` is excluded here: bars are spawned per-monitor on the GTK
        // thread (see `gtk_loop::run`'s `auto_show_bar` path), because monitor
        // enumeration requires `gdk::Display::default()` which is GTK-thread-only.
        let dispatcher_for_autoshow = setup.ipc_dispatcher.clone();
        let widgets_to_show: Vec<String> = setup
            .config
            .widget
            .iter()
            .filter(|w| w.auto_show && w.view != "widgets/bar")
            .map(|w| w.view.clone())
            .collect();
        // Track whether widgets/bar should be auto-shown per-monitor on the GTK thread.
        let auto_show_bar = setup
            .config
            .widget
            .iter()
            .any(|w| w.auto_show && w.view == "widgets/bar");
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

        let _exit_code = crate::gtk_loop::run(
            &app,
            window_rx,
            setup.ipc_dispatcher,
            setup.theme_store,
            worker.handle.clone(),
            setup.event_tx.clone(),
            window_request_tx,
            auto_show_bar,
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

    let theme_store = Arc::new(ThemeStore::new(active_theme));
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
            match quantum_infrastructure::HyprlandWindowsProvider::new(client_arc).await {
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
        match quantum_infrastructure::DeclarativeShellProvider::new(
            provider_config.clone(),
            shell_executor.clone(),
        ) {
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

    // Tray providers — each registered with graceful fallback. If the
    // underlying service is missing the provider still publishes an
    // unavailable state so the frontend has a uniform contract.
    match UpowerBatteryProvider::connect().await {
        Ok(p) => {
            let p = Arc::new(p);
            registry
                .register(p.id().clone(), p as Arc<dyn quantum_domain::ProviderSource>)
                .await;
            info!("Registered UpowerBatteryProvider");
        }
        Err(e) => tracing::warn!(error = ?e, "UpowerBatteryProvider unavailable"),
    }
    match NetworkManagerProvider::connect().await {
        Ok(p) => {
            let p = Arc::new(p);
            registry
                .register(p.id().clone(), p as Arc<dyn quantum_domain::ProviderSource>)
                .await;
            info!("Registered NetworkManagerProvider");
        }
        Err(e) => tracing::warn!(error = ?e, "NetworkManagerProvider unavailable"),
    }
    match BluezProvider::connect().await {
        Ok(p) => {
            let p = Arc::new(p);
            registry
                .register(p.id().clone(), p as Arc<dyn quantum_domain::ProviderSource>)
                .await;
            info!("Registered BluezProvider");
        }
        Err(e) => tracing::warn!(error = ?e, "BluezProvider unavailable"),
    }
    match PowerProfilesDaemonProvider::connect().await {
        Ok(p) => {
            let p = Arc::new(p);
            registry
                .register(p.id().clone(), p as Arc<dyn quantum_domain::ProviderSource>)
                .await;
            info!("Registered PowerProfilesDaemonProvider");
        }
        Err(e) => tracing::warn!(error = ?e, "PowerProfilesDaemonProvider unavailable"),
    }
    match LogindBrightnessProvider::connect(tokio::runtime::Handle::current()).await {
        Ok(p) => {
            let p = Arc::new(p);
            registry
                .register(p.id().clone(), p as Arc<dyn quantum_domain::ProviderSource>)
                .await;
            info!("Registered LogindBrightnessProvider");
        }
        Err(e) => tracing::warn!(error = ?e, "LogindBrightnessProvider unavailable"),
    }
    match PulseAudioProvider::connect(tokio::runtime::Handle::current()).await {
        Ok(p) => {
            let p = Arc::new(p);
            registry
                .register(p.id().clone(), p as Arc<dyn quantum_domain::ProviderSource>)
                .await;
            info!("Registered PulseAudioProvider");
        }
        Err(e) => tracing::warn!(error = ?e, "PulseAudioProvider unavailable"),
    }

    // Action-only system_power provider (shutdown/restart/suspend/hibernate/lock).
    let lock_command_cfg = config
        .system_power
        .as_ref()
        .and_then(|sp| sp.lock_command.clone());
    match SystemPowerProvider::connect(lock_command_cfg).await {
        Ok(p) => {
            let p = Arc::new(p);
            registry
                .register(p.id().clone(), p as Arc<dyn quantum_domain::ProviderSource>)
                .await;
            info!("Registered SystemPowerProvider");
        }
        Err(e) => tracing::warn!(error = ?e, "SystemPowerProvider unavailable"),
    }

    // Use cases
    let search_use_case = Arc::new(SearchUseCase::new(registry.clone()));
    let launch_action_use_case = Arc::new(LaunchActionUseCase::new(registry.clone()));
    let list_providers_use_case = Arc::new(ListProvidersUseCase::new(registry.clone()));
    let reload_theme_use_case = Arc::new(ReloadThemeUseCase::new(
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
    ] {
        let _ = subscribe_provider_use_case.execute(id.into()).await;
    }

    // Use the window host passed in from main (GtkWindowHost when running with
    // GTK, DummyWindowHost when headless).
    let open_view_use_case = Arc::new(OpenViewUseCase::new(window_host));

    let schedule_action_use_case =
        Arc::new(ScheduleActionUseCase::new(launch_action_use_case.clone()));
    let dispatcher = Arc::new(AppDispatcher::new(
        search_use_case,
        launch_action_use_case,
        list_providers_use_case,
        reload_theme_use_case,
        open_view_use_case,
        subscribe_provider_use_case,
        query_provider_use_case,
        schedule_action_use_case,
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
}
