//! quantum-ui

pub mod error;
pub mod app;
pub mod bridge;

pub use app::QuantumApp;
pub use error::{UiError, Result};
pub use bridge::register_bridge;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
