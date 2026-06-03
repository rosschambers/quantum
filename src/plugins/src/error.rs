use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum PluginsError {
    #[error("plugin I/O error: {0}")]
    Io(String),
    #[error("plugin config parse error: {0}")]
    ConfigParse(String),
}

impl From<std::io::Error> for PluginsError {
    fn from(e: std::io::Error) -> Self {
        PluginsError::Io(e.to_string())
    }
}

impl From<toml::de::Error> for PluginsError {
    fn from(e: toml::de::Error) -> Self {
        PluginsError::ConfigParse(e.to_string())
    }
}
