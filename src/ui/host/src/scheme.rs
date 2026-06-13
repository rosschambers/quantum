//! Custom quantum:// URI scheme handler for theme bundles.

use gio::MemoryInputStream;
use glib::Bytes;
use quantum_domain::ports::ThemeStore;
use std::sync::Arc;
use webkit6::{URISchemeRequest, WebContext};

/// Register the quantum:// URI scheme on `WebContext::default()`. Use this
/// convenience from binaries that don't want to take a webkit6 dependency
/// just to call into the default context.
pub fn register_quantum_scheme_on_default(theme_store: Arc<dyn ThemeStore>) {
    let Some(ctx) = WebContext::default() else {
        tracing::error!("WebContext::default() returned None; quantum:// scheme not registered");
        return;
    };
    register_quantum_scheme(&ctx, theme_store);
}

/// Register the quantum:// URI scheme.
/// Routes:
/// - quantum://theme/<name>/views/<view>/... -> bytes from ThemeStore
/// - quantum://assets/... -> asset bytes
pub fn register_quantum_scheme(context: &WebContext, theme_store: Arc<dyn ThemeStore>) {
    context.register_uri_scheme("quantum", move |request: &URISchemeRequest| {
        let Some(uri) = request.uri() else {
            tracing::warn!("quantum:// request with no URI");
            return;
        };
        let uri_str = uri.as_str();
        tracing::debug!("quantum:// request: {uri_str}");

        let Some(parsed) = parse_quantum_uri(uri_str) else {
            tracing::warn!("malformed quantum URI: {uri_str}");
            let mut error = glib::Error::new(
                glib::FileError::Noent,
                &format!("malformed quantum URI: {uri_str}"),
            );
            request.finish_error(&mut error);
            return;
        };

        let path_for_mime = parsed.path();
        let is_plugin = matches!(parsed, QuantumPath::Plugin { .. });
        let bytes = match parsed {
            QuantumPath::Theme { name, path } => theme_store.get_file(&name, &path),
            QuantumPath::Assets { path } => theme_store.get_asset(&path),
            QuantumPath::Plugin { name, path } => theme_store.get_plugin_file(&name, &path),
        };

        let Some(bytes_data) = bytes else {
            tracing::warn!("quantum:// not found: {uri_str}");
            let mut error =
                glib::Error::new(glib::FileError::Noent, &format!("not found: {uri_str}"));
            request.finish_error(&mut error);
            return;
        };
        tracing::debug!("quantum:// served {uri_str}: {} bytes", bytes_data.len());

        // For HTML files, inject resolved tokens. Match the extension
        // case-insensitively so user themes with `Index.HTML` or similar
        // capitalized filenames still receive the token injection.
        let is_html = path_for_mime
            .rsplit('.')
            .next()
            .map(|s| s.eq_ignore_ascii_case("html"))
            .unwrap_or(false);
        let final_bytes = if is_html {
            let html = String::from_utf8_lossy(&bytes_data).into_owned();
            let tokens = theme_store.resolved_tokens();
            let mut injected = inject_tokens(&html, &tokens);
            if is_plugin {
                injected = inject_plugin_client(&injected);
            }
            injected.into_bytes()
        } else {
            bytes_data
        };

        let mime = content_type_for(&path_for_mime);
        let bytes_len = final_bytes.len() as i64;
        let stream = MemoryInputStream::from_bytes(&Bytes::from_owned(final_bytes));
        request.finish(&stream, bytes_len, Some(mime));
    });
}

/// Parsed quantum URI path.
#[derive(Debug, Clone)]
enum QuantumPath {
    Theme { name: String, path: String },
    Assets { path: String },
    Plugin { name: String, path: String },
}

impl QuantumPath {
    fn path(&self) -> String {
        match self {
            QuantumPath::Theme { path, .. } => path.clone(),
            QuantumPath::Assets { path } => path.clone(),
            QuantumPath::Plugin { path, .. } => path.clone(),
        }
    }
}

/// Returns true when `s` is a safe path segment: non-empty, not `.`, and
/// not `..`. Used by `parse_quantum_uri` to refuse URIs that try to escape
/// their theme or assets sandbox via traversal segments.
fn is_safe_segment(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".."
}

/// Parse a quantum:// URI into its components.
/// - quantum://theme/name/path/to/file -> Theme { name: "name", path: "path/to/file" }
/// - quantum://assets/path/to/file -> Assets { path: "path/to/file" }
///
/// Rejects any URI whose path contains `.`, `..`, or empty segments
/// (other than the implicit empty segment after `quantum://`). This stops
/// `quantum://theme/default/../../etc/passwd` style traversal attacks
/// before they ever reach the theme store.
fn parse_quantum_uri(uri: &str) -> Option<QuantumPath> {
    if !uri.starts_with("quantum://") {
        return None;
    }

    let rest = &uri[10..]; // Skip "quantum://"
    let parts: Vec<&str> = rest.split('/').collect();

    match parts.as_slice() {
        ["theme", name, rest @ ..]
            if is_safe_segment(name)
                && !rest.is_empty()
                && rest.iter().all(|seg| is_safe_segment(seg)) =>
        {
            Some(QuantumPath::Theme {
                name: (*name).to_string(),
                path: rest.join("/"),
            })
        }
        ["assets", rest @ ..]
            if !rest.is_empty() && rest.iter().all(|seg| is_safe_segment(seg)) =>
        {
            Some(QuantumPath::Assets {
                path: rest.join("/"),
            })
        }
        ["plugin", name, rest @ ..]
            if is_safe_segment(name)
                && !rest.is_empty()
                && rest.iter().all(|seg| is_safe_segment(seg)) =>
        {
            Some(QuantumPath::Plugin {
                name: (*name).to_string(),
                path: rest.join("/"),
            })
        }
        _ => None,
    }
}

/// Map a path's file extension to a MIME type. The extension is lowercased
/// once so user themes with capitalized filenames (e.g. `Index.HTML`,
/// `Logo.PNG`) get the right `Content-Type` instead of falling through to
/// `application/octet-stream`.
fn content_type_for(path: &str) -> &'static str {
    let ext = path
        .rsplit('.')
        .next()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "jpeg" | "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

/// JavaScript bootstrap injected into plugin HTML before the page's
/// own scripts run. Wraps the existing bridge globals
/// (`window.webkit.messageHandlers.quantum`, `window.__quantum_resolve`,
/// `window.__quantum_reject`, `window.__quantum_notify`) into a tiny
/// `window.quantum.createClient()` surface so plugin script.js can call
/// IPC methods without bundling `@quantum/client`.
const PLUGIN_CLIENT_SCRIPT: &str = r#"<script>
(function () {
    var responseCallbacks = [];
    var notifyCallbacks = [];
    var nextId = 1;
    var pending = {};

    function ensureGlobals() {
        if (!window.__quantum_resolve) {
            window.__quantum_resolve = function (id, result) {
                var pc = pending[id];
                if (!pc) return;
                delete pending[id];
                pc.resolve(result);
            };
        }
        if (!window.__quantum_reject) {
            window.__quantum_reject = function (id, err) {
                var pc = pending[id];
                if (!pc) return;
                delete pending[id];
                pc.reject(err || { code: -32603, message: 'Internal error' });
            };
        }
        if (!window.__quantum_notify) {
            window.__quantum_notify = function (channel, payload) {
                notifyCallbacks.forEach(function (cb) { cb(channel, payload); });
            };
        }
    }

    function createClient() {
        ensureGlobals();
        return {
            call: function (method, params) {
                return new Promise(function (resolve, reject) {
                    var transport = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.quantum;
                    if (!transport) {
                        reject({ code: -32000, message: 'quantum bridge unavailable' });
                        return;
                    }
                    var id = nextId++;
                    pending[id] = { resolve: resolve, reject: reject };
                    transport.postMessage(JSON.stringify({ jsonrpc: '2.0', id: id, method: method, params: params }));
                });
            },
            subscribe: function (channel, callback) {
                ensureGlobals();
                var listener = function (ch, payload) {
                    if (ch === channel) callback(payload);
                };
                notifyCallbacks.push(listener);
                return function () {
                    var idx = notifyCallbacks.indexOf(listener);
                    if (idx !== -1) notifyCallbacks.splice(idx, 1);
                };
            },
        };
    }

    window.quantum = { createClient: createClient };
})();
</script>
"#;

/// Insert the plugin-client bootstrap script into HTML. Placed after the
/// `</head>` tag if one exists, otherwise immediately after the first `>`
/// (which closes a doctype declaration or `<html ...>`). Falling-back
/// prepends to the body for HTML fragments with no recognisable structure.
pub fn inject_plugin_client(html: &str) -> String {
    if let Some(idx) = html.find("</head>") {
        let mut out = String::with_capacity(html.len() + PLUGIN_CLIENT_SCRIPT.len());
        out.push_str(&html[..idx]);
        out.push_str(PLUGIN_CLIENT_SCRIPT);
        out.push_str(&html[idx..]);
        return out;
    }
    if let Some(idx) = html.find('>') {
        let mut out = String::with_capacity(html.len() + PLUGIN_CLIENT_SCRIPT.len());
        out.push_str(&html[..=idx]);
        out.push_str(PLUGIN_CLIENT_SCRIPT);
        out.push_str(&html[idx + 1..]);
        return out;
    }
    let mut out = String::with_capacity(html.len() + PLUGIN_CLIENT_SCRIPT.len());
    out.push_str(PLUGIN_CLIENT_SCRIPT);
    out.push_str(html);
    out
}

/// Inject resolved tokens into HTML by replacing the placeholder with CSS.
pub fn inject_tokens(html: &str, tokens: &std::collections::HashMap<String, String>) -> String {
    let css = quantum_domain::tokens_to_css(tokens);
    html.replace("/* QUANTUM_TOKENS */", &css)
}

/// Build a JavaScript statement that replaces the live token stylesheet's
/// content with `css`.
///
/// Pages embed `<style id="quantum-tokens">...</style>`. At serve time
/// [`inject_tokens`] fills it; for a live theme reload the daemon instead
/// pushes this statement into an already-open WebView so the page recolors
/// without a reload.
///
/// The statement is guarded: it looks the element up and only assigns when it
/// exists, mirroring the `__quantum_notify` guard used elsewhere, because the
/// reload event can arrive before the page has finished parsing its `<head>`.
/// `css` is JSON-encoded with `serde_json::to_string`, producing a quoted,
/// fully escaped JavaScript string literal (newlines, quotes, and backslashes
/// in token values are handled), so the CSS text can never break out of the
/// literal or inject script.
pub fn token_push_js(css: &str) -> String {
    let quoted = serde_json::to_string(css).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        "var __quantum_tokens_el = document.getElementById('quantum-tokens'); \
         if (__quantum_tokens_el) {{ __quantum_tokens_el.textContent = {quoted}; }}"
    )
}

#[cfg(test)]
mod inject_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn replaces_placeholder_with_css() {
        let html = r#"<style id="quantum-tokens">/* QUANTUM_TOKENS */</style>"#;
        let mut t = HashMap::new();
        t.insert("color-bg".into(), "#fff".into());
        let out = inject_tokens(html, &t);
        assert!(out.contains("--color-bg: #fff;"));
        assert!(!out.contains("/* QUANTUM_TOKENS */"));
    }

    #[test]
    fn html_without_placeholder_unchanged() {
        let html = "<html><body>no placeholder</body></html>";
        let out = inject_tokens(html, &HashMap::new());
        assert_eq!(out, html);
    }

    #[test]
    fn token_push_js_guards_missing_element() {
        let js = token_push_js(":root {}\n");
        assert!(js.contains("getElementById('quantum-tokens')"));
        assert!(js.contains("if (__quantum_tokens_el)"));
        assert!(js.contains("textContent"));
    }

    #[test]
    fn token_push_js_json_encodes_css() {
        // A newline and a double quote must be escaped so the CSS sits
        // inside a single valid JavaScript string literal.
        let js = token_push_js(":root {\n  --x: \"q\";\n}\n");
        assert!(js.contains("\\n"));
        assert!(js.contains("\\\""));
        // The raw newline must not survive into the emitted statement.
        assert!(!js.contains("\n  --x"));
    }

    #[test]
    fn plugin_client_injected_before_page_scripts() {
        let html = r#"<html><head><title>x</title></head><body><script src="script.js"></script></body></html>"#;
        let out = inject_plugin_client(html);
        assert!(out.contains("window.quantum"));
        assert!(out.contains("createClient"));
        let qpos = out.find("window.quantum").expect("present");
        let spos = out.find("script.js").expect("present");
        assert!(
            qpos < spos,
            "window.quantum injection must precede page scripts"
        );
    }

    #[test]
    fn plugin_client_injected_when_no_head() {
        let html = r#"<html><body><script src="x.js"></script></body></html>"#;
        let out = inject_plugin_client(html);
        assert!(out.contains("window.quantum"));
        let qpos = out.find("window.quantum").expect("present");
        let spos = out.find("x.js").expect("present");
        assert!(qpos < spos, "injection must precede the body script");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_registers_without_error() {
        // Smoke test: the function signature is correct and doesn't panic.
        // Actual WebKit integration testing requires a display server.
    }

    #[test]
    fn parses_theme_uri_correctly() {
        let uri = "quantum://theme/default/views/launcher/index.html";
        let parts: Vec<&str> = uri.split('/').collect();
        // Expected: ["quantum:", "", "theme", "default", "views", "launcher", "index.html"]
        assert_eq!(parts[2], "theme");
        assert_eq!(parts[3], "default");
        assert_eq!(parts[4], "views");
    }

    #[test]
    fn parses_assets_uri_correctly() {
        let uri = "quantum://assets/icons/app.png";
        let parts: Vec<&str> = uri.split('/').collect();
        assert_eq!(parts[2], "assets");
    }

    #[test]
    fn parse_quantum_uri_theme_path() {
        let uri = "quantum://theme/default/views/launcher/index.html";
        let parsed = parse_quantum_uri(uri).expect("parse");
        match parsed {
            QuantumPath::Theme { name, path } => {
                assert_eq!(name, "default");
                assert_eq!(path, "views/launcher/index.html");
            }
            _ => panic!("expected Theme variant"),
        }
    }

    #[test]
    fn parse_quantum_uri_assets_path() {
        let uri = "quantum://assets/icons/app.png";
        let parsed = parse_quantum_uri(uri).expect("parse");
        match parsed {
            QuantumPath::Assets { path } => {
                assert_eq!(path, "icons/app.png");
            }
            _ => panic!("expected Assets variant"),
        }
    }

    #[test]
    fn parse_quantum_uri_rejects_malformed() {
        assert!(parse_quantum_uri("http://example.com").is_none());
        assert!(parse_quantum_uri("quantum://").is_none());
        assert!(parse_quantum_uri("quantum://theme//index.html").is_none());
    }

    #[test]
    fn content_type_html() {
        assert_eq!(content_type_for("index.html"), "text/html");
    }

    #[test]
    fn content_type_css() {
        assert_eq!(content_type_for("style.css"), "text/css");
    }

    #[test]
    fn content_type_js() {
        assert_eq!(content_type_for("app.js"), "application/javascript");
    }

    #[test]
    fn content_type_svg() {
        assert_eq!(content_type_for("icon.svg"), "image/svg+xml");
    }

    #[test]
    fn content_type_png() {
        assert_eq!(content_type_for("image.png"), "image/png");
    }

    #[test]
    fn content_type_unknown() {
        assert_eq!(content_type_for("data.xyz"), "application/octet-stream");
    }

    #[test]
    fn content_type_uppercase_extension() {
        // Regression: capitalized extensions used to fall through to the
        // octet-stream default because the match arms were case-sensitive.
        assert_eq!(content_type_for("FILE.HTML"), "text/html");
        assert_eq!(content_type_for("Logo.PNG"), "image/png");
        assert_eq!(content_type_for("Style.Css"), "text/css");
    }

    #[test]
    fn parse_rejects_dotdot_in_theme_path() {
        // Regression: a malicious theme could embed a script that fetched
        // `quantum://theme/default/../../etc/passwd` and `PathBuf::join`
        // would happily walk out of the themes directory.
        assert!(parse_quantum_uri("quantum://theme/default/../etc/passwd").is_none());
        assert!(parse_quantum_uri("quantum://theme/default/../../etc/passwd").is_none());
    }

    #[test]
    fn parse_rejects_dotdot_in_assets_path() {
        assert!(parse_quantum_uri("quantum://assets/../etc/passwd").is_none());
        assert!(parse_quantum_uri("quantum://assets/icons/../../etc/passwd").is_none());
    }

    #[test]
    fn parse_rejects_dot_segment() {
        // `.` segments are also disallowed: they would let an attacker
        // bypass naive `..` filters while still resolving to the same path.
        assert!(parse_quantum_uri("quantum://theme/default/./views/launcher/index.html").is_none());
        assert!(parse_quantum_uri("quantum://assets/./icons/app.png").is_none());
    }

    #[test]
    fn parse_quantum_uri_plugin_path() {
        let uri = "quantum://plugin/moon-distance/views/moon-widget/index.html";
        let parsed = parse_quantum_uri(uri).expect("parse");
        match parsed {
            QuantumPath::Plugin { name, path } => {
                assert_eq!(name, "moon-distance");
                assert_eq!(path, "views/moon-widget/index.html");
            }
            _ => panic!("expected Plugin variant"),
        }
    }

    #[test]
    fn parse_rejects_dotdot_in_plugin_path() {
        assert!(parse_quantum_uri("quantum://plugin/moon/../etc/passwd").is_none());
        assert!(parse_quantum_uri("quantum://plugin/../../etc/passwd").is_none());
    }

    #[test]
    fn parse_rejects_plugin_without_path() {
        assert!(parse_quantum_uri("quantum://plugin/moon").is_none());
        assert!(parse_quantum_uri("quantum://plugin/").is_none());
        assert!(parse_quantum_uri("quantum://plugin//x.html").is_none());
    }
}
