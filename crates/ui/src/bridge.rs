//! WebKit script message bridge to Dispatcher.

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use quantum_application::dispatcher::Dispatcher;

/// Message sent from JavaScript to Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    pub id: u64,
    pub method: String,
    pub params: Value,
}

/// Register the bridge message handler on a WebView.
/// 
/// This would normally integrate with webkit6::WebView to register script message handlers.
/// For now, this is a placeholder that validates the message structure works.
pub fn register_bridge(_webview: &webkit6::WebView, _dispatcher: Arc<Dispatcher>) {
    // In a real implementation, we would:
    // 1. Get or create the WebContext
    // 2. Register a URI scheme handler for quantum://
    // 3. Set up script message handler for IPC
    // 
    // For now, this validates the bridge infrastructure is in place.
}

/// Handle an incoming bridge message.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
