//! Window management for Quantum UI.

pub mod panel;
pub mod widget;

pub use panel::PanelWindow;
pub use widget::WidgetWindow;

use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use gtk4::gdk;
use quantum_domain::{ports::ThemeStore, EventEnvelope};

use crate::bridge::json_to_js_expression;
use crate::dispatcher::IpcDispatcher;

/// Build the batched JavaScript statement that pushes the theme store's
/// current resolved tokens into a live `#quantum-tokens` stylesheet.
///
/// Shared by [`WidgetWindow`] and [`PanelWindow`]: on a `theme.reloaded`
/// event each re-resolves tokens from the store (rather than trusting the
/// event payload's CSS) so a theme switch and an in-place token edit are
/// handled identically. `resolved_tokens()` is synchronous and safe to call
/// off the GTK thread, so this runs inside the Tokio event-subscriber task.
/// The result is already passed through `json_to_js_expression`, so it can be
/// sent straight down the window's JS mpsc/batch channel.
pub(crate) fn theme_reload_push_js(store: &Arc<dyn ThemeStore>) -> String {
    let css = quantum_domain::tokens_to_css(&store.resolved_tokens());
    json_to_js_expression(&crate::scheme::token_push_js(&css))
}

/// Disable WebKit's default browser context menu on a Quantum WebView.
///
/// These surfaces are a widget/launcher host, not a browser, so the native
/// right-click menu (Back / Forward / Reload / Stop / "Open Link" ...) is
/// meaningless and looks out of place. Suppressing it lets each view define its
/// own right-click behavior through the DOM `contextmenu` event (the bar's tray
/// indicators already do) or have none at all.
///
/// When the inspector is enabled (`QUANTUM_INSPECTOR=1`) the menu is left in
/// place so "Inspect Element" stays available for debugging.
pub(crate) fn suppress_browser_context_menu(webview: &webkit6::WebView, inspector_enabled: bool) {
    use webkit6::prelude::WebViewExt;
    // Returning true from the handler tells WebKit not to display its menu.
    webview.connect_context_menu(move |_view, _menu, _hit_test| !inspector_enabled);
}

/// The shared host context every window constructor needs: the IPC
/// dispatcher and theme store, the Tokio runtime handle and broadcast sender
/// for event forwarding, and the optional monitor to pin the surface to.
///
/// Bundling these into one value keeps the per-window constructors down to a
/// handful of window-specific arguments. The struct is single-threaded by
/// construction (it carries non-`Send` GTK types) and is consumed by each
/// `new` call, mirroring how the constructors already move their `Arc` clones
/// and the `Handle`/`Sender` into the window.
pub(crate) struct WindowContext {
    pub dispatcher: Arc<dyn IpcDispatcher>,
    pub theme_store: Arc<dyn ThemeStore>,
    pub runtime: Handle,
    pub event_tx: broadcast::Sender<EventEnvelope>,
    pub monitor: Option<gdk::Monitor>,
}

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
