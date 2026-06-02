use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced by the DBus helpers. Two variants today: transient
/// transport failures and "service is not running" cases. Callers map
/// these to their own error type via `From` impls.
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum DbusError {
    #[error("dbus transport error: {0}")]
    Transport(String),
    #[error("dbus service unavailable: {0}")]
    ServiceUnavailable(String),
}

impl From<zbus::Error> for DbusError {
    fn from(e: zbus::Error) -> Self {
        DbusError::Transport(e.to_string())
    }
}

impl From<zbus::fdo::Error> for DbusError {
    fn from(e: zbus::fdo::Error) -> Self {
        DbusError::Transport(e.to_string())
    }
}
