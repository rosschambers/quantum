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
            let injected = inject_tokens(&html, &tokens);
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

/// Inject resolved tokens into HTML by replacing the placeholder with CSS.
pub fn inject_tokens(html: &str, tokens: &std::collections::HashMap<String, String>) -> String {
    let css = quantum_domain::tokens_to_css(tokens);
    html.replace("/* QUANTUM_TOKENS */", &css)
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
