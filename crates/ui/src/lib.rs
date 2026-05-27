//! quantum-ui

pub mod app;
pub mod bridge;
pub mod error;
pub mod scheme;
pub mod window_host;
pub mod windows;

pub use app::QuantumApp;
pub use bridge::register_bridge;
pub use error::{Result, UiError};
pub use scheme::{register_quantum_scheme, ThemePort};
pub use window_host::{DummyWindowHost, GtkWindowHost};
pub use windows::LauncherWindow;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
