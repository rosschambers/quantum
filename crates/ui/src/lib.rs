//! quantum-ui

pub mod error;
pub mod app;
pub mod bridge;
pub mod scheme;
pub mod windows;
pub mod window_host;

pub use app::QuantumApp;
pub use error::{UiError, Result};
pub use bridge::register_bridge;
pub use scheme::{register_quantum_scheme, ThemePort};
pub use windows::LauncherWindow;
pub use window_host::{GtkWindowHost, WindowMode};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
