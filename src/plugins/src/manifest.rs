use crate::error::PluginsError;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const MINIMUM_INTERVAL_SECS: u64 = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub scripts: HashMap<String, ScriptConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptConfig {
    pub interval: Duration,
    pub channel: Option<String>,
}

pub fn parse_manifest(text: &str) -> Result<Manifest, PluginsError> {
    if text.trim().is_empty() {
        return Ok(Manifest::default());
    }

    #[derive(Deserialize)]
    struct RawManifest {
        #[serde(default)]
        scripts: HashMap<String, RawScript>,
    }

    #[derive(Deserialize)]
    struct RawScript {
        interval: Option<u64>,
        channel: Option<String>,
    }

    let raw: RawManifest =
        toml::from_str(text).map_err(|e| PluginsError::ConfigParse(e.to_string()))?;

    let mut scripts = HashMap::new();
    for (name, raw_script) in raw.scripts {
        let interval_secs = raw_script.interval.ok_or_else(|| {
            PluginsError::ConfigParse(format!("missing interval for script '{name}'"))
        })?;
        if interval_secs < MINIMUM_INTERVAL_SECS {
            return Err(PluginsError::ConfigParse(format!(
                "interval {interval_secs}s below minimum {MINIMUM_INTERVAL_SECS}s for script '{name}'"
            )));
        }
        scripts.insert(
            name,
            ScriptConfig {
                interval: Duration::from_secs(interval_secs),
                channel: raw_script.channel,
            },
        );
    }

    Ok(Manifest { scripts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty_manifest() {
        let result = parse_manifest("").expect("empty is valid");
        assert!(result.scripts.is_empty());
    }

    #[test]
    fn whitespace_only_input_returns_empty_manifest() {
        let result = parse_manifest("   \n  \t\n").expect("whitespace is valid");
        assert!(result.scripts.is_empty());
    }

    #[test]
    fn valid_single_script() {
        let toml = "[scripts.moon-distance]\ninterval = 3600\n";
        let result = parse_manifest(toml).expect("valid");
        let script = result.scripts.get("moon-distance").expect("present");
        assert_eq!(script.interval, Duration::from_secs(3600));
        assert!(script.channel.is_none());
    }

    #[test]
    fn multiple_scripts_with_optional_channel() {
        let toml = "[scripts.moon]\ninterval = 3600\n\n[scripts.weather]\ninterval = 600\nchannel = \"weather.live\"\n";
        let result = parse_manifest(toml).expect("valid");
        assert_eq!(result.scripts.len(), 2);
        assert_eq!(
            result.scripts.get("moon").unwrap().interval,
            Duration::from_secs(3600)
        );
        assert_eq!(result.scripts.get("moon").unwrap().channel, None);
        assert_eq!(
            result.scripts.get("weather").unwrap().interval,
            Duration::from_secs(600)
        );
        assert_eq!(
            result.scripts.get("weather").unwrap().channel.as_deref(),
            Some("weather.live")
        );
    }

    #[test]
    fn interval_at_minimum_is_accepted() {
        let toml = "[scripts.short]\ninterval = 5\n";
        let result = parse_manifest(toml).expect("5s is the minimum");
        assert_eq!(
            result.scripts.get("short").unwrap().interval,
            Duration::from_secs(5)
        );
    }

    #[test]
    fn interval_below_minimum_is_rejected() {
        let toml = "[scripts.spam]\ninterval = 1\n";
        let err = parse_manifest(toml).expect_err("must reject");
        let msg = format!("{err}");
        assert!(msg.contains("interval"), "error mentions 'interval': {msg}");
        assert!(
            msg.contains("spam") || msg.contains("minimum"),
            "error mentions script name or minimum: {msg}"
        );
    }

    #[test]
    fn missing_interval_is_rejected() {
        let toml = "[scripts.broken]\nchannel = \"x\"\n";
        let err = parse_manifest(toml).expect_err("must reject");
        let msg = format!("{err}");
        assert!(
            msg.contains("interval") || msg.contains("broken"),
            "error mentions interval or script name: {msg}"
        );
    }

    #[test]
    fn unknown_top_level_fields_are_ignored() {
        let toml = "description = \"ignored\"\narbitrary = 42\n\n[scripts.x]\ninterval = 60\n";
        let result = parse_manifest(toml).expect("extra fields fine");
        assert_eq!(result.scripts.len(), 1);
        assert_eq!(
            result.scripts.get("x").unwrap().interval,
            Duration::from_secs(60)
        );
    }

    #[test]
    fn malformed_toml_returns_error() {
        let toml = "this is not toml [[[";
        let err = parse_manifest(toml).expect_err("malformed");
        let msg = format!("{err}");
        assert!(
            msg.to_lowercase().contains("parse")
                || msg.to_lowercase().contains("expected")
                || msg.to_lowercase().contains("invalid"),
            "expected a parse-related message: {msg}"
        );
    }
}
