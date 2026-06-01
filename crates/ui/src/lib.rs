//! quantum-ui

pub mod bridge;
pub mod dispatcher;
pub mod error;
pub mod messages;
pub mod registry;
pub mod scheme;
pub mod window_host;
pub mod windows;

pub use bridge::register_bridge;
pub use dispatcher::IpcDispatcher;
pub use error::{Result, UiError};
pub use messages::WindowRequest;
pub use quantum_domain::tokens_to_css;
pub use registry::{ManagedWindow, ManagedWindowConstructor, WindowOps, WindowRegistry};
pub use scheme::{register_quantum_scheme, register_quantum_scheme_on_default};
pub use window_host::{DummyWindowHost, GtkWindowHost};
pub use windows::PanelWindow;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
