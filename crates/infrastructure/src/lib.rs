//! quantum-infrastructure

pub mod config;
pub mod error;
pub mod hyprland;
pub mod providers;
pub mod registry;
pub mod shell;

pub use config::ConfigStore;
pub use error::InfrastructureError;
pub use hyprland::HyprlandSocketClient;
pub use providers::{
    DeclarativeShellProvider, DesktopAppsProvider, HyprlandWindowsProvider, ShellCommandProvider,
};
pub use registry::InMemoryProviderRegistry;
pub use shell::TokioShellExecutor;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
