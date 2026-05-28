//! quantum-ui

pub mod app;
pub mod bridge;
pub mod dispatcher;
pub mod error;
pub mod messages;
pub mod registry;
pub mod scheme;
pub mod window_host;
pub mod windows;

pub use app::QuantumApp;
pub use bridge::register_bridge;
pub use dispatcher::IpcDispatcher;
pub use error::{Result, UiError};
pub use messages::{WindowEvent, WindowRequest};
pub use registry::{ManagedWindow, ManagedWindowConstructor, WindowOps, WindowRegistry};
pub use scheme::register_quantum_scheme;
pub use window_host::{DummyWindowHost, GtkWindowHost};
pub use windows::LauncherWindow;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
