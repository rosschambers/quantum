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
    ReloadThemeUseCase, SearchUseCase,
};
use quantum_domain::{DomainError, EventBus, ProviderId, ProviderSource};
use quantum_infrastructure::ipc::server::{
    DispatchError, DispatchResult, Dispatcher as IpcDispatcher,
};
use quantum_infrastructure::{
    config::ConfigStore, providers::DesktopAppsProvider, registry::InMemoryProviderRegistry,
    shell::TokioShellExecutor, theme::ThemeStore, EventEnvelope, HyprlandSocketClient,
    ShellCommandProvider, UnixSocketServer,
};
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
            Err(err) => Err(DispatchError::new(-32603, err.to_string())),
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
                code: -32603,
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
        let payload_json: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
        let _ = self.tx.send(EventEnvelope {
            channel: event.to_string(),
            payload: payload_json,
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

    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("quantum-worker")
        .build()?;
    let worker = runtime::spawn_worker(tokio_runtime);

    // Set up window host (GTK or dummy).
    let (window_host, window_rx) = if headless {
        let dummy = Arc::new(DummyWindowHost::new()) as Arc<dyn quantum_domain::ports::WindowHost>;
        // No channel in headless; use a never-receiving fake.
        let (_unused_tx, rx) = tokio::sync::mpsc::unbounded_channel::<quantum_ui::WindowRequest>();
        (dummy, rx)
    } else {
        let (host, rx) = quantum_ui::GtkWindowHost::new();
        (
            Arc::new(host) as Arc<dyn quantum_domain::ports::WindowHost>,
            rx,
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
        let _exit_code = crate::gtk_loop::run(
            &app,
            window_rx,
            setup.ipc_dispatcher,
            setup.theme_store,
            worker.handle.clone(),
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
    #[allow(dead_code)]
    event_tx: tokio::sync::broadcast::Sender<EventEnvelope>,
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
    match HyprlandSocketClient::new() {
        Ok(client) => {
            match quantum_infrastructure::HyprlandWindowsProvider::new(Arc::new(client)).await {
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

    // Use cases
    let search_use_case = Arc::new(SearchUseCase::new(registry.clone()));
    let launch_action_use_case = Arc::new(LaunchActionUseCase::new(registry.clone()));
    let list_providers_use_case = Arc::new(ListProvidersUseCase::new(registry.clone()));
    let reload_theme_use_case = Arc::new(ReloadThemeUseCase::new(
        theme_store.clone() as Arc<dyn quantum_domain::ThemeStore>,
        event_bus.clone(),
    ));

    // Use the window host passed in from main (GtkWindowHost when running with
    // GTK, DummyWindowHost when headless).
    let open_view_use_case = Arc::new(OpenViewUseCase::new(window_host));

    let dispatcher = Arc::new(AppDispatcher::new(
        search_use_case,
        launch_action_use_case,
        list_providers_use_case,
        reload_theme_use_case,
        open_view_use_case,
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
    })
}

async fn run_signal_loop(socket_path: std::path::PathBuf) {
    let socket_path_clone = socket_path.clone();
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("signal setup");
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .expect("signal setup");

    info!("Running daemon (headless mode)");

    tokio::select! {
        _ = sigterm.recv() => info!("Received SIGTERM"),
        _ = sigint.recv() => info!("Received SIGINT"),
        _ = tokio::signal::ctrl_c() => info!("Received Ctrl+C"),
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
