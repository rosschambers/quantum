//! WebKit script message bridge to Dispatcher.

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use quantum_application::dispatcher::Dispatcher;
use webkit6::prelude::*;

/// Message sent from JavaScript to Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    id: u64,
    method: String,
    params: Value,
}

/// Register the bridge message handler on a WebView.
pub fn register_bridge(webview: &webkit6::WebView, dispatcher: Arc<Dispatcher>) {
    let dispatcher_clone = dispatcher.clone();
    webview.register_uri_scheme_as_display_name("quantum");

    // Register script message handler
    let handler = webkit6::UserContentManager::new();

    // Clone dispatcher for the closure
    let disp = dispatcher_clone.clone();
    handler.connect_script_message_received(move |_, msg| {
        if let Some(body) = msg.js_value().and_then(|v| v.to_string().ok()) {
            handle_message(&body, &disp);
        }
    });

    webview.user_content_manager().add_script_message_handler(&handler, "quantum");
}

/// Handle an incoming bridge message.
fn handle_message(json_str: &str, dispatcher: &Arc<Dispatcher>) {
    match serde_json::from_str::<BridgeMessage>(json_str) {
        Ok(msg) => {
            let dispatcher = dispatcher.clone();
            let id = msg.id;
            let method = msg.method.clone();
            let params = msg.params.clone();

            // Spawn async handler on Tokio runtime
            tokio::spawn(async move {
                match dispatcher.dispatch(&method, params).await {
                    Ok(result) => {
                        // Send response back via JavaScript
                        let json = serde_json::to_string(&result)
                            .unwrap_or_else(|_| json!(null).to_string());
                        let js = format!(
                            "window.__quantum_resolve({}, {})",
                            id,
                            escape_for_js(&json)
                        );
                        // In real implementation, we'd evaluate this on the webview
                        // For now, we log it
                        eprintln!("Would send JS: {}", js);
                    }
                    Err(e) => {
                        let error_json = serde_json::to_string(&e)
                            .unwrap_or_else(|_| json!({"error": "serialization failed"}).to_string());
                        let js = format!(
                            "window.__quantum_reject({}, {})",
                            id,
                            escape_for_js(&error_json)
                        );
                        eprintln!("Would send error JS: {}", js);
                    }
                }
            });
        }
        Err(e) => {
            eprintln!("Failed to parse bridge message: {}", e);
        }
    }
}

/// Escape JSON string for safe injection into JavaScript.
fn escape_for_js(json: &str) -> String {
    format!("\"{}\"", json.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bridge_message() {
        let msg_json = r#"{"id": 1, "method": "system.status", "params": {}}"#;
        let msg: BridgeMessage = serde_json::from_str(msg_json).unwrap();
        assert_eq!(msg.id, 1);
        assert_eq!(msg.method, "system.status");
    }

    #[test]
    fn escapes_json_for_javascript() {
        let json = r#"{"key": "value with \"quotes\""}"#;
        let escaped = escape_for_js(json);
        assert!(escaped.starts_with('"'));
        assert!(escaped.ends_with('"'));
        // Should properly escape backslashes and quotes
        assert!(escaped.contains("\\\\"));
    }

    #[test]
    fn message_roundtrip() {
        let original = BridgeMessage {
            id: 42,
            method: "search".to_string(),
            params: json!({"text": "test"}),
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: BridgeMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 42);
        assert_eq!(parsed.method, "search");
    }
}
