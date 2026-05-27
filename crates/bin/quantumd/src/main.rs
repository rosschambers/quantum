use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use quantum_application::{
    Dispatcher, LaunchActionUseCase, ListProvidersUseCase, OpenViewUseCase, ReloadThemeUseCase,
    SearchUseCase,
};
use quantum_infrastructure::{
    config::ConfigStore, providers::DesktopAppsProvider, registry::InMemoryProviderRegistry,
    shell::TokioShellExecutor, theme::ThemeStore, HyprlandSocketClient, IpcServer,
    ShellCommandProvider,
};
use quantum_ui::DummyWindowHost;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("Starting quantumd v{}", env!("CARGO_PKG_VERSION"));

    // Determine if headless mode
    let headless = std::env::args().any(|arg| arg == "--headless");
    let socket_override = std::env::args()
        .find_map(|arg| {
            if arg.starts_with("--socket=") {
                Some(arg.strip_prefix("--socket=").unwrap().to_string())
            } else {
                None
            }
        })
        .or_else(|| {
            std::env::args()
                .zip(std::env::args().skip(1))
                .find_map(|(a, b)| {
                    if a == "--socket" {
                        Some(b)
                    } else {
                        None
                    }
                })
        });

    // Load configuration
    let config = ConfigStore::load().unwrap_or_else(|err| {
        tracing::warn!("Failed to load config: {}. Using defaults.", err);
        ConfigStore::default()
    });

    let active_theme = config.general.as_ref().and_then(|g| g.active_theme.clone());

    // Create infrastructure components
    let theme_store = Arc::new(ThemeStore::new(active_theme));
    let shell_executor = Arc::new(TokioShellExecutor::new());
    let registry = Arc::new(InMemoryProviderRegistry::new());

    // Register built-in providers
    // Desktop apps provider
    if let Ok(provider) = DesktopAppsProvider::new(shell_executor.clone()).await {
        info!("Registered DesktopAppsProvider");
        registry
            .register(Arc::new(provider) as Arc<dyn quantum_domain::ports::ProviderSource>)
            .await;
    } else {
        tracing::warn!("Failed to create DesktopAppsProvider");
    }

    // Shell command provider
    let shell_cmd = Arc::new(ShellCommandProvider::new(shell_executor.clone()));
    registry.register(shell_cmd).await;
    info!("Registered ShellCommandProvider");

    // Hyprland provider (optional)
    if let Ok(client) = HyprlandSocketClient::new() {
        if let Ok(provider) =
            quantum_infrastructure::HyprlandWindowsProvider::new(Arc::new(client)).await
        {
            registry.register(Arc::new(provider)).await;
            info!("Registered HyprlandWindowsProvider");
        }
    } else {
        tracing::warn!("Hyprland not available. Continuing without Hyprland support.");
    }

    // Register declarative shell providers from config
    if let Some(providers) = &config.providers {
        for provider_config in providers {
            match quantum_infrastructure::DeclarativeShellProvider::new(
                provider_config.clone(),
                shell_executor.clone(),
            )
            .await
            {
                Ok(provider) => {
                    info!("Registered DeclarativeShellProvider: {}", provider_config.id);
                    registry.register(Arc::new(provider)).await;
                }
                Err(err) => {
                    tracing::warn!("Failed to register {}: {}", provider_config.id, err);
                }
            }
        }
    }

    // Create application layer components
    let search_use_case = Arc::new(SearchUseCase::new(registry.clone()));
    let launch_action_use_case = Arc::new(LaunchActionUseCase::new(registry.clone()));
    let list_providers_use_case = Arc::new(ListProvidersUseCase::new(registry.clone()));
    let reload_theme_use_case = Arc::new(ReloadThemeUseCase::new(theme_store.clone()));

    // Use dummy window host (real one would require GTK initialization)
    let window_host: Arc<dyn quantum_domain::ports::WindowHost> =
        Arc::new(DummyWindowHost::new());

    let open_view_use_case = Arc::new(OpenViewUseCase::new(window_host.clone()));

    // Create dispatcher
    let dispatcher = Arc::new(Dispatcher::new(
        search_use_case,
        launch_action_use_case,
        list_providers_use_case,
        reload_theme_use_case,
        open_view_use_case,
    ));

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
        // Try to connect to see if daemon is running
        if tokio::net::UnixStream::connect(&socket_path).await.is_err() {
            // Daemon not running, remove stale socket
            if let Err(err) = std::fs::remove_file(&socket_path) {
                tracing::warn!("Failed to remove stale socket: {}", err);
            }
        } else {
            eprintln!("quantum is already running");
            std::process::exit(1);
        }
    }

    // Start IPC server
    let ipc_server = Arc::new(IpcServer::new(socket_path.clone(), dispatcher.clone()));
    let server_handle = {
        let server = ipc_server.clone();
        tokio::spawn(async move {
            if let Err(err) = server.serve().await {
                tracing::error!("IPC server error: {}", err);
            }
        })
    };

    info!("IPC server listening on {}", socket_path.display());

    // Signal handling for graceful shutdown
    let socket_path_clone = socket_path.clone();
    let signal_task = tokio::spawn(async move {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("signal setup");
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("signal setup");

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl+C");
            }
        }

        // Clean up socket file
        if let Err(err) = std::fs::remove_file(&socket_path_clone) {
            tracing::warn!("Failed to remove socket file: {}", err);
        }

        std::process::exit(0);
    });

    // For now, just run in headless mode
    info!("Running daemon (headless mode)");
    let _ = signal_task.await;

    Ok(())
}
