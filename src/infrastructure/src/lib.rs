//! quantum-infrastructure

pub mod error;
pub mod registry;
pub mod shell;

pub use error::InfrastructureError;
pub use quantum_config::ConfigStore;
pub use quantum_hyprland::HyprlandSocketClient;
pub use quantum_ipc::{EventEnvelope, UnixSocketServer};
pub use quantum_theme::ThemeStore;
pub use registry::InMemoryProviderRegistry;
pub use shell::TokioShellExecutor;

// Compat re-exports of providers from quantum-providers. Lets existing
// consumers continue to `use quantum_infrastructure::providers::{...}` until
// they migrate to `quantum_providers::{...}` directly.
pub mod providers {
    pub use quantum_providers::*;
}

pub use quantum_providers::{
    DeclarativeShellProvider, DesktopAppsProvider, HyprlandActiveWindowProvider,
    HyprlandWindowsProvider, MprisProvider, ProcStatsProvider, ShellCommandProvider,
};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
