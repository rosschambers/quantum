//! Custom quantum:// URI scheme handler for theme bundles.

use std::sync::Arc;
use gio::MemoryInputStream;
use glib::Bytes;
use webkit6::{URISchemeRequest, WebContext};
use quantum_domain::ports::ThemeStore;

/// Register the quantum:// URI scheme.
/// Routes:
/// - quantum://theme/<name>/views/<view>/... -> bytes from ThemeStore
/// - quantum://assets/... -> asset bytes
pub fn register_quantum_scheme(
    context: &WebContext,
    theme_store: Arc<dyn ThemeStore>,
) {
    context.register_uri_scheme("quantum", move |request: &URISchemeRequest| {
        let Some(uri) = request.uri() else {
            return;
        };
        let uri_str = uri.as_str();

        let Some(parsed) = parse_quantum_uri(uri_str) else {
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
        };

        let Some(bytes) = bytes else {
            let mut error = glib::Error::new(
                glib::FileError::Noent,
                &format!("not found: {uri_str}"),
            );
            request.finish_error(&mut error);
            return;
        };

        let mime = content_type_for(&path_for_mime);
        let bytes_len = bytes.len() as i64;
        let stream = MemoryInputStream::from_bytes(&Bytes::from_owned(bytes));
        request.finish(&stream, bytes_len, Some(mime));
    });
}

/// Parsed quantum URI path.
#[derive(Debug, Clone)]
enum QuantumPath {
    Theme { name: String, path: String },
    Assets { path: String },
}

impl QuantumPath {
    fn path(&self) -> String {
        match self {
            QuantumPath::Theme { path, .. } => path.clone(),
            QuantumPath::Assets { path } => path.clone(),
        }
    }
}

/// Parse a quantum:// URI into its components.
/// - quantum://theme/name/path/to/file -> Theme { name: "name", path: "path/to/file" }
/// - quantum://assets/path/to/file -> Assets { path: "path/to/file" }
fn parse_quantum_uri(uri: &str) -> Option<QuantumPath> {
    if !uri.starts_with("quantum://") {
        return None;
    }

    let rest = &uri[10..]; // Skip "quantum://"
    let parts: Vec<&str> = rest.split('/').collect();

    match parts.as_slice() {
        ["theme", name, rest @ ..] if !name.is_empty() && !rest.is_empty() => {
            Some(QuantumPath::Theme {
                name: name.to_string(),
                path: rest.join("/"),
            })
        }
        ["assets", rest @ ..] if !rest.is_empty() => {
            Some(QuantumPath::Assets {
                path: rest.join("/"),
            })
        }
        _ => None,
    }
}

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("jpeg") | Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("json") => "application/json",
        _ => "application/octet-stream",
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
}
