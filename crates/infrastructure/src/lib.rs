//! quantum-infrastructure

pub mod error;
pub mod registry;
pub mod shell;

pub use error::InfrastructureError;
pub use registry::InMemoryProviderRegistry;
pub use shell::TokioShellExecutor;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
