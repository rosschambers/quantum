//! Window management for Quantum UI.

pub mod panel;
pub mod widget;

pub use panel::PanelWindow;
pub use widget::WidgetWindow;

/// Resolve the `quantum://` URI a view loads from its canonical name.
///
/// Plugin views take the shape `plugin/<plugin>/<view>` and resolve to
/// `quantum://plugin/<plugin>/views/<view>/index.html` (the scheme handler
/// routes those through [`quantum_domain::ports::ThemeStore::get_plugin_file`],
/// which inserts `dist/` and applies user-disk-over-embedded precedence).
/// Theme-hosted views (the only remaining one is the clock widget) stay on
/// the `quantum://theme/default/views/<name>/index.html` route.
///
/// A malformed plugin name (no `/` after the `plugin/` prefix) falls back to
/// a theme lookup that 404s cleanly rather than panicking.
pub(crate) fn resolve_view_uri(canonical_name: &str) -> String {
    if let Some(plugin_path) = canonical_name.strip_prefix("plugin/") {
        if let Some((plugin, view)) = plugin_path.split_once('/') {
            return format!("quantum://plugin/{plugin}/views/{view}/index.html");
        }
    }
    format!("quantum://theme/default/views/{canonical_name}/index.html")
}

#[cfg(test)]
mod tests {
    use super::resolve_view_uri;

    #[test]
    fn plugin_view_resolves_to_plugin_uri() {
        assert_eq!(
            resolve_view_uri("plugin/launcher/launcher"),
            "quantum://plugin/launcher/views/launcher/index.html"
        );
        assert_eq!(
            resolve_view_uri("plugin/power-menu/power-menu"),
            "quantum://plugin/power-menu/views/power-menu/index.html"
        );
    }

    #[test]
    fn theme_view_resolves_to_theme_uri() {
        assert_eq!(
            resolve_view_uri("widgets/clock"),
            "quantum://theme/default/views/widgets/clock/index.html"
        );
    }

    #[test]
    fn malformed_plugin_name_falls_back_to_theme_uri() {
        // `plugin/onlyone` has no second path segment; resolving it as a
        // theme path yields a clean 404 instead of a panic.
        assert_eq!(
            resolve_view_uri("plugin/onlyone"),
            "quantum://theme/default/views/plugin/onlyone/index.html"
        );
    }
}
