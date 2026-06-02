use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced by hyprctl client + hyprland event socket. Callers
/// map these to their own error type via `From` impls if needed.
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum HyprlandError {
    #[error("hyprland I/O error: {0}")]
    Io(String),
    #[error("malformed hyprland JSON: {0}")]
    Serde(String),
    #[error("hyprland socket unreachable")]
    Unreachable,
}

impl From<std::io::Error> for HyprlandError {
    fn from(e: std::io::Error) -> Self {
        HyprlandError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for HyprlandError {
    fn from(e: serde_json::Error) -> Self {
        HyprlandError::Serde(e.to_string())
    }
}
