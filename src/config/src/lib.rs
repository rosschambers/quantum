//! quantum-config: config.toml loader.

pub mod error;
pub mod loader;

pub use error::ConfigError;
pub use loader::{
    Config, ConfigStore, GeneralConfig, MatchTemplate, ProviderConfig, SystemPowerConfig,
    WidgetConfig,
};
