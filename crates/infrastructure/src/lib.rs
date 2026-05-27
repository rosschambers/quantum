//! quantum-infrastructure

pub mod error;
pub mod registry;

pub use error::InfrastructureError;
pub use registry::InMemoryProviderRegistry;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
