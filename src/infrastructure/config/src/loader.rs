use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::RwLock;

use quantum_domain::Action;

use crate::error::ConfigError;

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

/// Configuration for a widget.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WidgetConfig {
    pub view: String,
    #[serde(default)]
    pub auto_show: bool,
}

/// Main configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub provider: Vec<ProviderConfig>,
    #[serde(default)]
    pub widget: Vec<WidgetConfig>,
    #[serde(default)]
    pub system_power: Option<SystemPowerConfig>,
    #[serde(default)]
    pub commands: Option<CommandsConfig>,
}

/// Configuration for external commands the daemon shells out to.
///
/// Each value is a shell-style command line; tokens are split on whitespace
/// honouring quoting. When unset the daemon probes the standard tool on PATH.
/// `clipboard_watcher` overrides the `wl-paste --watch` clipboard watcher;
/// `clipboard_copy` overrides the `wl-copy` writer used to place entries back
/// on the clipboard.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandsConfig {
    #[serde(default)]
    pub clipboard_watcher: Option<String>,
    #[serde(default)]
    pub clipboard_copy: Option<String>,
}

/// Configuration for the `system_power` provider.
///
/// `lock_command` is a shell-style command line; tokens are split on
/// whitespace honouring quoting. When unset the provider probes
/// `hyprlock`, `swaylock`, `gtklock`, then falls back to
/// `loginctl lock-session`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemPowerConfig {
    #[serde(default)]
    pub lock_command: Option<String>,
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
            active_theme: Some("sycamore".to_string()),
        }
    }
}

/// Configuration store for loading and managing config files.
pub struct ConfigStore {
    config: RwLock<Config>,
}

impl ConfigStore {
    /// Create a new config store and load from file.
    pub async fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path();

        let config = if path.exists() {
            let content =
                std::fs::read_to_string(&path).map_err(|e| ConfigError::Io(e.to_string()))?;

            toml::from_str(&content)
                .map_err(|e| ConfigError::ConfigParse(format!("TOML parse error: {}", e)))?
        } else {
            Config {
                general: GeneralConfig::default(),
                provider: Vec::new(),
                widget: Vec::new(),
                system_power: None,
                commands: None,
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
                widget: Vec::new(),
                system_power: None,
                commands: None,
            }),
        };

        let config = store.get_config().await;
        assert_eq!(config.general.active_theme, Some("sycamore".to_string()));
    }

    #[tokio::test]
    async fn config_store_get_active_theme() {
        let store = ConfigStore {
            config: RwLock::new(Config {
                general: GeneralConfig {
                    active_theme: Some("dark".to_string()),
                },
                provider: Vec::new(),
                widget: Vec::new(),
                system_power: None,
                commands: None,
            }),
        };

        let theme = store.get("active_theme").await;
        assert_eq!(theme, Some("dark".to_string()));
    }

    #[test]
    fn parses_widget_entries() {
        let toml = r#"
            [[widget]]
            view = "widgets/bar"
            auto_show = true

            [[widget]]
            view = "widgets/clock"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.widget.len(), 2);
        assert_eq!(config.widget[0].view, "widgets/bar");
        assert!(config.widget[0].auto_show);
        assert!(!config.widget[1].auto_show);
    }

    #[test]
    fn parses_system_power_section() {
        let toml = r#"
            [system_power]
            lock_command = "swaylock --color 000"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        let sp = config.system_power.expect("system_power present");
        assert_eq!(sp.lock_command.as_deref(), Some("swaylock --color 000"));
    }

    #[test]
    fn config_without_system_power_section_parses() {
        let toml = "";
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.system_power.is_none());
    }

    #[test]
    fn parses_commands_section() {
        let toml = r#"
            [commands]
            clipboard_watcher = "wl-paste --watch"
            clipboard_copy = "wl-copy"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        let commands = config.commands.expect("commands present");
        assert_eq!(
            commands.clipboard_watcher.as_deref(),
            Some("wl-paste --watch")
        );
        assert_eq!(commands.clipboard_copy.as_deref(), Some("wl-copy"));
    }

    #[test]
    fn config_without_commands_section_parses() {
        let toml = "";
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.commands.is_none());
    }
}
