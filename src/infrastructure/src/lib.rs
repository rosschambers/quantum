//! quantum-infrastructure

pub mod error;
pub mod ipc;
pub mod providers;
pub mod registry;
pub mod shell;

pub use error::InfrastructureError;
pub use ipc::{EventEnvelope, UnixSocketServer};
pub use providers::{
    DeclarativeShellProvider, DesktopAppsProvider, HyprlandActiveWindowProvider,
    HyprlandWindowsProvider, MprisProvider, ProcStatsProvider, ShellCommandProvider,
};
pub use quantum_config::ConfigStore;
pub use quantum_hyprland::HyprlandSocketClient;
pub use quantum_theme::ThemeStore;
pub use registry::InMemoryProviderRegistry;
pub use shell::TokioShellExecutor;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
