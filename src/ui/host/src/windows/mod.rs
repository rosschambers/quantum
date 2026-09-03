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

/// The result of classifying a navigation URI for policy enforcement.
///
/// Used by [`install_navigation_policy`] to decide what to do when a WebView
/// attempts to navigate to a URI. The classification is extracted as a pure
/// function ([`classify_navigation_uri`]) so it can be unit-tested without GTK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NavigationDecision {
    /// Allow the WebView to navigate (quantum:// or empty URI).
    Allow,
    /// Block the navigation and open the URI in the user's default browser.
    OpenExternal(String),
    /// Block the navigation silently (unknown or unsafe schemes).
    Block,
}

/// Classify a navigation URI into a [`NavigationDecision`].
///
/// - `quantum://` URIs and empty or blank URIs are allowed (internal navigation).
/// - `http://` and `https://` URIs are opened externally in the default browser.
/// - All other schemes (ftp, javascript, data, file, and similar) are blocked.
pub(crate) fn classify_navigation_uri(uri: &str) -> NavigationDecision {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return NavigationDecision::Allow;
    }
    if trimmed.starts_with("quantum://") {
        return NavigationDecision::Allow;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return NavigationDecision::OpenExternal(trimmed.to_owned());
    }
    NavigationDecision::Block
}

/// Spawn `xdg-open` for the given URI in a fire-and-forget manner.
///
/// stdin, stdout, and stderr are all directed to null so the child process
/// is fully detached from the daemon. The process is not killed when the
/// `Child` handle is dropped (`kill_on_drop` is false by default for
/// `std::process::Command`).
fn open_url_externally(uri: &str) {
    match std::process::Command::new("xdg-open")
        .arg(uri)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_child) => {
            tracing::debug!(uri, "spawned xdg-open for external URI");
        }
        Err(error) => {
            tracing::warn!(uri, %error, "failed to spawn xdg-open for external URI");
        }
    }
}

/// Install a navigation policy handler on a Quantum WebView.
///
/// Quantum views are a widget/launcher host, not a browser. When the rendered
/// content contains clickable links (for example Markdown in the file viewer),
/// the default WebKit behavior is to navigate the WebView itself — which is
/// wrong: the user expects `https://` links to open in their browser.
///
/// This handler intercepts every navigation policy decision:
/// - `quantum://` and empty URIs are allowed (internal navigation).
/// - `http://` and `https://` URIs are blocked and opened externally via
///   `xdg-open`.
/// - All other schemes are blocked silently.
///
/// Non-navigation decisions (resource loads, and similar) are left to WebKit's
/// default policy.
pub(crate) fn install_navigation_policy(webview: &webkit6::WebView) {
    use glib::prelude::Cast;
    use webkit6::prelude::*;

    webview.connect_decide_policy(|_view, decision, decision_type| {
        if decision_type != webkit6::PolicyDecisionType::NavigationAction
            && decision_type != webkit6::PolicyDecisionType::NewWindowAction
        {
            return false;
        }

        let Some(navigation_decision) =
            decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
        else {
            decision.ignore();
            return true;
        };

        let uri = navigation_decision
            .navigation_action()
            .and_then(|mut action| action.request())
            .and_then(|request| request.uri())
            .map(|gstring| gstring.to_string())
            .unwrap_or_default();

        match classify_navigation_uri(&uri) {
            NavigationDecision::Allow => {
                decision.use_();
            }
            NavigationDecision::OpenExternal(external_uri) => {
                decision.ignore();
                open_url_externally(&external_uri);
            }
            NavigationDecision::Block => {
                decision.ignore();
            }
        }

        true
    });
}

/// Connect a `web-process-terminated` handler that reloads the view when its
/// render process crashes or is OOM-killed.
///
/// Without this handler, a dead WebKitWebProcess leaves the view's surface
/// blank forever (the "stranded blank bar" failure mode). The handler logs the
/// reason and reloads the same `quantum://` URI — since views are stateless
/// Svelte bundles served over the custom scheme, a reload restores them fully.
///
/// `TerminatedByApi` (explicit teardown via `terminate_web_process()` on
/// `destroy_on_dismiss` views) is excluded — those views are being deliberately
/// torn down and should not reload.
pub(crate) fn install_web_process_crash_handler(webview: &webkit6::WebView, view_name: &str) {
    use webkit6::prelude::*;
    let name = view_name.to_string();
    webview.connect_web_process_terminated(move |view, reason| {
        match reason {
            webkit6::WebProcessTerminationReason::Crashed => {
                tracing::warn!(view = %name, "WebKitWebProcess crashed; reloading view");
            }
            webkit6::WebProcessTerminationReason::ExceededMemoryLimit => {
                tracing::warn!(
                    view = %name,
                    "WebKitWebProcess exceeded memory limit; reloading view"
                );
            }
            webkit6::WebProcessTerminationReason::TerminatedByApi => {
                // Deliberate teardown (destroy_on_dismiss); do not reload.
                tracing::debug!(view = %name, "WebKitWebProcess terminated by API");
                return;
            }
            _ => {
                tracing::warn!(view = %name, ?reason, "WebKitWebProcess terminated; reloading view");
            }
        }
        // Reload the view's current URI. If it's still the quantum:// URI we
        // loaded originally, this restores the view. If somehow blank, fall
        // back to reload() which re-requests the last committed URI.
        if let Some(uri) = view.uri() {
            let uri_str = uri.as_str();
            if !uri_str.is_empty() && uri_str != "about:blank" {
                view.load_uri(uri_str);
                return;
            }
        }
        view.reload();
    });
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
    pub web_context: webkit6::WebContext,
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

/// Inject view arguments into a WebView as `window.__quantum_args` on load.
///
/// When a view is opened with optional JSON arguments, expose them to the
/// frontend as `window.__quantum_args` (set to `null` if no args were provided).
/// Inject on `LoadEvent::Committed`: at that point the document object exists
/// but no page script has run yet. The handler fires on every load, so the
/// args persist across navigation and reloads.
pub(crate) fn inject_view_args(webview: &webkit6::WebView, args: Option<serde_json::Value>) {
    use webkit6::prelude::WebViewExt;
    let json_string = match &args {
        Some(value) => serde_json::to_string(value).unwrap_or_else(|_| "null".into()),
        None => "null".into(),
    };
    let js = format!("window.__quantum_args = {};", json_string);

    // Inject immediately in case the page is already loaded (the
    // LoadEvent::Committed may have already fired before this function
    // was called). A no-op if the document does not exist yet.
    let webview_immediate = webview.clone();
    let js_immediate = js.clone();
    WebViewExt::evaluate_javascript(
        &webview_immediate,
        &js_immediate,
        None,
        None,
        gtk4::gio::Cancellable::NONE,
        |_| {},
    );

    // Also connect a handler for future loads/reloads so the args
    // persist across navigation.
    let webview_clone = webview.clone();
    webview.connect_load_changed(move |_view, event| {
        if event == webkit6::LoadEvent::Committed {
            WebViewExt::evaluate_javascript(
                &webview_clone,
                &js,
                None,
                None,
                gtk4::gio::Cancellable::NONE,
                |_| {},
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::resolve_view_uri;
    use super::{classify_navigation_uri, NavigationDecision};

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

    #[test]
    fn quantum_scheme_is_allowed() {
        assert_eq!(
            classify_navigation_uri("quantum://plugin/launcher/views/launcher/index.html"),
            NavigationDecision::Allow,
        );
        assert_eq!(
            classify_navigation_uri("quantum://theme/default/views/widgets/clock/index.html"),
            NavigationDecision::Allow,
        );
    }

    #[test]
    fn empty_uri_is_allowed() {
        assert_eq!(classify_navigation_uri(""), NavigationDecision::Allow);
        assert_eq!(classify_navigation_uri("   "), NavigationDecision::Allow);
    }

    #[test]
    fn https_opens_externally() {
        assert_eq!(
            classify_navigation_uri("https://example.com/page"),
            NavigationDecision::OpenExternal("https://example.com/page".to_owned()),
        );
        assert_eq!(
            classify_navigation_uri("https://github.com/user/repo#readme"),
            NavigationDecision::OpenExternal("https://github.com/user/repo#readme".to_owned()),
        );
    }

    #[test]
    fn http_opens_externally() {
        assert_eq!(
            classify_navigation_uri("http://example.com"),
            NavigationDecision::OpenExternal("http://example.com".to_owned()),
        );
    }

    #[test]
    fn ftp_scheme_is_blocked() {
        assert_eq!(
            classify_navigation_uri("ftp://files.example.com/pub"),
            NavigationDecision::Block,
        );
    }

    #[test]
    fn javascript_scheme_is_blocked() {
        assert_eq!(
            classify_navigation_uri("javascript:alert(1)"),
            NavigationDecision::Block,
        );
    }

    #[test]
    fn data_scheme_is_blocked() {
        assert_eq!(
            classify_navigation_uri("data:text/html,<h1>hi</h1>"),
            NavigationDecision::Block,
        );
    }

    #[test]
    fn file_scheme_is_blocked() {
        assert_eq!(
            classify_navigation_uri("file:///etc/passwd"),
            NavigationDecision::Block,
        );
    }
}
