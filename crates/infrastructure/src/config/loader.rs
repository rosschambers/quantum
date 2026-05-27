use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::RwLock;

use quantum_domain::Action;

use crate::InfrastructureError;

/// Configuration for a declarative shell provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub refresh: Option<String>,
    #[serde(default)]
    pub parse: Option<String>,
    #[serde(default)]
    pub match_template: Option<MatchTemplate>,
    #[serde(default)]
    pub action: Option<Action>,
}

/// Template for matching output lines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchTemplate {
    pub title: Option<String>,
    pub subtitle: Option<String>,
}

/// Main configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub provider: Vec<ProviderConfig>,
}

/// General configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default)]
    pub active_theme: Option<String>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            active_theme: Some("default".to_string()),
        }
    }
}

/// Configuration store for loading and managing config files.
pub struct ConfigStore {
    config: RwLock<Config>,
}

impl ConfigStore {
    /// Create a new config store and load from file.
    pub async fn load() -> Result<Self, InfrastructureError> {
        let path = Self::config_path();

        let config = if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| InfrastructureError::Io(e.to_string()))?;

            toml::from_str(&content).map_err(|e| {
                InfrastructureError::ConfigParse(format!("TOML parse error: {}", e))
            })?
        } else {
            Config {
                general: GeneralConfig::default(),
                provider: Vec::new(),
            }
        };

        Ok(Self {
            config: RwLock::new(config),
        })
    }

    /// Get the path to the config file.
    fn config_path() -> PathBuf {
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));

        PathBuf::from(config_home).join("quantum/config.toml")
    }

    /// Get the current configuration.
    pub async fn get_config(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Get a specific value.
    pub async fn get(&self, key: &str) -> Option<String> {
        let config = self.config.read().await;
        match key {
            "active_theme" => config.general.active_theme.clone(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_config() {
        let toml_str = r#"
[general]
active_theme = "default"

[[provider]]
id = "pacman-updates"
command = ["sh", "-c", "checkupdates"]
refresh = "300s"
parse = "lines"
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.active_theme, Some("default".to_string()));
        assert_eq!(config.provider.len(), 1);
        assert_eq!(config.provider[0].id, "pacman-updates");
    }

    #[test]
    fn parse_invalid_config_fails() {
        let toml_str = r#"
[general
invalid syntax
"#;

        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn config_store_load_default() {
        let store = ConfigStore {
            config: RwLock::new(Config {
                general: GeneralConfig::default(),
                provider: Vec::new(),
            }),
        };

        let config = store.get_config().await;
        assert_eq!(
            config.general.active_theme,
            Some("default".to_string())
        );
    }

    #[tokio::test]
    async fn config_store_get_active_theme() {
        let store = ConfigStore {
            config: RwLock::new(Config {
                general: GeneralConfig {
                    active_theme: Some("dark".to_string()),
                },
                provider: Vec::new(),
            }),
        };

        let theme = store.get("active_theme").await;
        assert_eq!(theme, Some("dark".to_string()));
    }
}
