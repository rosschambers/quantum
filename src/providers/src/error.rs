use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced by concrete providers.
///
/// Each variant carries a stringified payload so the type is `Clone` and
/// `Serialize`. Use cases convert these to `DomainError` at the application
/// boundary; the IPC layer never sees a `ProvidersError` directly.
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProvidersError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("serde error: {0}")]
    Serde(String),
    #[error("config parse error: {0}")]
    ConfigParse(String),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("dbus transport error: {0}")]
    DbusTransport(String),
    #[error("spawn error: {0}")]
    Spawn(String),
}

impl From<std::io::Error> for ProvidersError {
    fn from(e: std::io::Error) -> Self {
        ProvidersError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for ProvidersError {
    fn from(e: serde_json::Error) -> Self {
        ProvidersError::Serde(e.to_string())
    }
}

impl From<quantum_dbus::DbusError> for ProvidersError {
    fn from(e: quantum_dbus::DbusError) -> Self {
        use quantum_dbus::DbusError as D;
        match e {
            D::Transport(s) => ProvidersError::DbusTransport(s),
            D::ServiceUnavailable(s) => ProvidersError::ServiceUnavailable(s),
        }
    }
}

impl From<quantum_hyprland::HyprlandError> for ProvidersError {
    fn from(e: quantum_hyprland::HyprlandError) -> Self {
        ProvidersError::Io(e.to_string())
    }
}
