//! quantum-ui

pub mod error;
pub mod app;

pub use app::QuantumApp;
pub use error::{UiError, Result};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
