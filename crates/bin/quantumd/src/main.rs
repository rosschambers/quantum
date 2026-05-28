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
use quantum_domain::{ProviderId, ProviderSource};
use quantum_infrastructure::ipc::server::{
    DispatchError, DispatchResult, Dispatcher as IpcDispatcher,
};
use quantum_infrastructure::{
    config::ConfigStore, providers::DesktopAppsProvider, registry::InMemoryProviderRegistry,
    shell::TokioShellExecutor, theme::ThemeStore, HyprlandSocketClient, InMemoryEventBus,
    ShellCommandProvider, UnixSocketServer,
};
use quantum_ui::DummyWindowHost;

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

    // All async setup happens on the worker.
    let setup = worker
        .handle
        .block_on(async { setup_daemon(socket_override).await })?;

    if headless {
        // Run signal loop on the worker, blocking the main thread.
        worker.handle.block_on(async move {
            run_signal_loop(setup.socket_path).await;
        });
    } else {
        // To be wired in Task 2.4.
        eprintln!("GTK mode not yet wired; pass --headless");
        return Ok(());
    }

    worker.shutdown();
    Ok(())
}

struct DaemonSetup {
    socket_path: std::path::PathBuf,
}

async fn setup_daemon(
    socket_override: Option<String>,
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
    let event_bus: Arc<dyn quantum_domain::EventBus> = Arc::new(InMemoryEventBus::new());

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

    let window_host: Arc<dyn quantum_domain::ports::WindowHost> = Arc::new(DummyWindowHost::new());
    let open_view_use_case = Arc::new(OpenViewUseCase::new(window_host.clone()));

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
        tokio::spawn(async move {
            if let Err(err) = server.serve(dispatcher).await {
                tracing::error!("IPC server error: {err}");
            }
        });
    }

    info!("IPC server listening on {}", socket_path.display());

    Ok(DaemonSetup { socket_path })
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
