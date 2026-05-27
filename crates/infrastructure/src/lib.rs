//! quantum-infrastructure

pub mod error;

pub use error::InfrastructureError;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
