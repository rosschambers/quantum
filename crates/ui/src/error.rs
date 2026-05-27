//! UI-layer errors.

use quantum_application::ApplicationError;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum UiError {
    #[error(transparent)]
    Application(#[from] ApplicationError),

    #[error("GTK error: {0}")]
    Gtk(String),

    #[error("WebKit error: {0}")]
    WebKit(String),
}

pub type Result<T> = std::result::Result<T, UiError>;
