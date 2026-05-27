//! quantum-infrastructure

pub mod error;
pub mod providers;
pub mod registry;
pub mod shell;

pub use error::InfrastructureError;
pub use providers::DesktopAppsProvider;
pub use registry::InMemoryProviderRegistry;
pub use shell::TokioShellExecutor;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
