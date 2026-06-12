//! Parser for per-view `view.toml` files. Every field is optional and
//! falls back to the `ViewDescriptor` defaults; unknown fields are
//! ignored so future keys do not break older daemons.

use crate::error::PluginsError;
use quantum_domain::{ViewAnchor, ViewDescriptor, ViewKind};
use serde::Deserialize;

pub fn parse_view_toml(text: &str) -> Result<ViewDescriptor, PluginsError> {
    if text.trim().is_empty() {
        return Ok(ViewDescriptor::default());
    }

    #[derive(Deserialize)]
    struct RawView {
        kind: Option<String>,
        per_monitor: Option<bool>,
        auto_show: Option<bool>,
        anchor: Option<String>,
        height: Option<u32>,
        width: Option<u32>,
        single_instance: Option<bool>,
    }

    let raw: RawView =
        toml::from_str(text).map_err(|e| PluginsError::ConfigParse(e.to_string()))?;

    let defaults = ViewDescriptor::default();

    let kind = match raw.kind.as_deref() {
        None => defaults.kind,
        Some("widget") => ViewKind::Widget,
        Some("panel") => ViewKind::Panel,
        Some("overlay") => ViewKind::Overlay,
        Some(other) => {
            return Err(PluginsError::ConfigParse(format!(
                "invalid kind '{other}': expected one of widget, panel, overlay"
            )));
        }
    };

    let anchor = match raw.anchor.as_deref() {
        None => defaults.anchor,
        Some("none") => ViewAnchor::None,
        Some("top") => ViewAnchor::Top,
        Some("bottom") => ViewAnchor::Bottom,
        Some(other) => {
            return Err(PluginsError::ConfigParse(format!(
                "invalid anchor '{other}': expected one of none, top, bottom"
            )));
        }
    };

    Ok(ViewDescriptor {
        kind,
        per_monitor: raw.per_monitor.unwrap_or(defaults.per_monitor),
        auto_show: raw.auto_show.unwrap_or(defaults.auto_show),
        anchor,
        height: raw.height,
        width: raw.width,
        single_instance: raw.single_instance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantum_domain::{ViewAnchor, ViewKind};

    #[test]
    fn empty_input_returns_default_descriptor() {
        let descriptor = parse_view_toml("").expect("empty is valid");
        assert_eq!(descriptor, ViewDescriptor::default());
    }

    #[test]
    fn whitespace_only_input_returns_default_descriptor() {
        let descriptor = parse_view_toml("   \n \t\n").expect("whitespace is valid");
        assert_eq!(descriptor, ViewDescriptor::default());
    }

    #[test]
    fn full_file_parses_every_field() {
        let toml = r#"
kind = "panel"
per_monitor = true
auto_show = true
anchor = "top"
height = 32
width = 600
single_instance = false
"#;
        let descriptor = parse_view_toml(toml).expect("valid full file");
        assert_eq!(
            descriptor,
            ViewDescriptor {
                kind: ViewKind::Panel,
                per_monitor: true,
                auto_show: true,
                anchor: ViewAnchor::Top,
                height: Some(32),
                width: Some(600),
                single_instance: Some(false),
            }
        );
    }

    #[test]
    fn partial_file_fills_defaults_for_missing_fields() {
        let toml = "kind = \"overlay\"\nheight = 400\n";
        let descriptor = parse_view_toml(toml).expect("partial file is valid");
        assert_eq!(
            descriptor,
            ViewDescriptor {
                kind: ViewKind::Overlay,
                height: Some(400),
                ..ViewDescriptor::default()
            }
        );
    }

    #[test]
    fn anchor_bottom_parses() {
        let descriptor = parse_view_toml("anchor = \"bottom\"\n").expect("valid anchor");
        assert_eq!(descriptor.anchor, ViewAnchor::Bottom);
    }

    #[test]
    fn anchor_none_parses() {
        let descriptor = parse_view_toml("anchor = \"none\"\n").expect("valid anchor");
        assert_eq!(descriptor.anchor, ViewAnchor::None);
    }

    #[test]
    fn invalid_kind_names_the_bad_value() {
        let err = parse_view_toml("kind = \"banana\"\n").expect_err("invalid kind");
        let message = format!("{err}");
        assert!(
            message.contains("banana"),
            "error must name the bad value: {message}"
        );
        assert!(
            message.contains("kind"),
            "error must name the field: {message}"
        );
        assert!(matches!(err, PluginsError::ConfigParse(_)));
    }

    #[test]
    fn invalid_anchor_names_the_bad_value() {
        let err = parse_view_toml("anchor = \"left\"\n").expect_err("invalid anchor");
        let message = format!("{err}");
        assert!(
            message.contains("left"),
            "error must name the bad value: {message}"
        );
        assert!(
            message.contains("anchor"),
            "error must name the field: {message}"
        );
        assert!(matches!(err, PluginsError::ConfigParse(_)));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let toml = "kind = \"widget\"\nfuture_option = \"whatever\"\nshiny = 9\n";
        let descriptor = parse_view_toml(toml).expect("unknown fields are fine");
        assert_eq!(descriptor.kind, ViewKind::Widget);
    }

    #[test]
    fn malformed_toml_returns_config_parse_error() {
        let err = parse_view_toml("not toml [[[").expect_err("malformed");
        assert!(matches!(err, PluginsError::ConfigParse(_)));
    }
}
