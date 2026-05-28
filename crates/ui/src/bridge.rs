//! WebKit script message bridge to Dispatcher.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::runtime::Handle;
use webkit6::{prelude::*, WebView};

use crate::IpcDispatcher;

/// Message sent from JavaScript to Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    pub id: u64,
    pub method: String,
    pub params: Value,
}

/// Register the bridge message handler on a WebView.
/// Wires WebKit script messages to the Tokio dispatcher with JS evaluation for responses.
pub fn register_bridge(
    webview: &WebView,
    dispatcher: Arc<dyn IpcDispatcher>,
    runtime: Handle,
) {
    let ucm = match webview.user_content_manager() {
        Some(mgr) => mgr,
        None => {
            tracing::error!("failed to get user content manager");
            return;
        }
    };

    ucm.register_script_message_handler("quantum", None);

    let webview_clone = webview.clone();

    ucm.connect_script_message_received(Some("quantum"), move |_ucm, msg| {
        // The message is a JavaScriptResult containing the JSON value from the script
        let json_str = match msg.to_string().as_str() {
            s if !s.is_empty() => s.to_string(),
            _ => {
                tracing::warn!("empty script message");
                return;
            }
        };

        let Ok(parsed): Result<BridgeMessage, _> = serde_json::from_str(&json_str) else {
            tracing::warn!("malformed bridge message: {json_str}");
            return;
        };

        let dispatcher = dispatcher.clone();
        let webview = webview_clone.clone();
        let id = parsed.id;
        let method = parsed.method.clone();
        let params = parsed.params.clone();

        // Create a channel to get the result back from the Tokio task
        let (tx, mut rx) = tokio::sync::oneshot::channel();

        // Spawn on Tokio to call the dispatcher
        runtime.spawn(async move {
            let result = dispatcher.dispatch(&method, params).await;
            let js = match result {
                Ok(value) => {
                    let payload =
                        serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
                    format!(
                        "window.__quantum_resolve({}, {})",
                        id,
                        escape_for_js(&payload)
                    )
                }
                Err(err) => {
                    let payload = serde_json::to_string(&serde_json::json!({
                        "code": err.code,
                        "message": err.message,
                    }))
                    .unwrap_or_else(|_| "{}".into());
                    format!(
                        "window.__quantum_reject({}, {})",
                        id,
                        escape_for_js(&payload)
                    )
                }
            };

            // Send the result back (ignore if receiver dropped)
            let _ = tx.send(js);
        });

        // Wait for the result in a non-blocking way on the GTK main thread
        glib::source::idle_add_local(move || {
            // Try to receive the JS (non-blocking)
            match rx.try_recv() {
                Ok(js) => {
                    webview.evaluate_javascript(
                        &js,
                        None,
                        None,
                        None::<&gio::Cancellable>,
                        |_| {},
                    );
                    glib::ControlFlow::Break
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    // Not ready yet, keep waiting
                    glib::ControlFlow::Continue
                }
                Err(_) => {
                    // Sender dropped or channel closed
                    glib::ControlFlow::Break
                }
            }
        });
    });
}

/// Escape JSON string for safe injection into JavaScript.
#[allow(dead_code)]
fn escape_for_js(json: &str) -> String {
    format!("\"{}\"", json.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
