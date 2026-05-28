//! WebKit script message bridge to Dispatcher.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::runtime::Handle;
use webkit6::{prelude::*, WebView};

use crate::IpcDispatcher;

/// Post-process a serialized JSON string so it is safe to splice into a
/// JavaScript program as an expression.
///
/// JSON permits the literal Unicode line separators U+2028 and U+2029 inside
/// string values, but JavaScript string literals do not — they terminate the
/// line. Replace them with their `\uXXXX` escapes. Everything else in valid
/// JSON is already a valid JS expression.
fn json_to_js_expression(json: &str) -> String {
    json.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Message sent from JavaScript to Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    pub id: u64,
    pub method: String,
    pub params: Value,
}

/// Register the bridge message handler on a WebView.
/// Wires WebKit script messages to the Tokio dispatcher with JS evaluation for responses.
pub fn register_bridge(webview: &WebView, dispatcher: Arc<dyn IpcDispatcher>, runtime: Handle) {
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

        // One-shot channel for the JS expression to evaluate once the
        // dispatcher completes. The receiver is awaited on the GLib main
        // context, so the GTK thread suspends until the value is ready
        // rather than busy-polling.
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();

        runtime.spawn(async move {
            let result = dispatcher.dispatch(&method, params).await;
            let js = match result {
                Ok(value) => {
                    let payload = serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
                    let payload = json_to_js_expression(&payload);
                    format!("window.__quantum_resolve({id}, {payload})")
                }
                Err(err) => {
                    let error_json = serde_json::to_string(&serde_json::json!({
                        "code": err.code,
                        "message": err.message,
                    }))
                    .unwrap_or_else(|_| "{}".into());
                    let error_json = json_to_js_expression(&error_json);
                    format!("window.__quantum_reject({id}, {error_json})")
                }
            };

            // Ignore send failure: the GTK side has gone away.
            let _ = tx.send(js);
        });

        // Drive the oneshot on the GLib main context. `spawn_local` yields
        // to the loop while awaiting, so there is no busy loop on the GTK
        // thread. A oneshot receiver is executor-agnostic — its waker is
        // supplied by whoever polls it (here, the GLib executor), so the
        // sender on the Tokio side correctly wakes this future.
        glib::MainContext::default().spawn_local(async move {
            if let Ok(js) = rx.await {
                webview.evaluate_javascript(&js, None, None, None::<&gio::Cancellable>, |_| {});
            }
        });
    });
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
    fn json_to_js_expression_handles_all_cases() {
        // Plain strings pass through unchanged.
        assert_eq!(json_to_js_expression("\"abc\""), "\"abc\"");
        // U+2028 (line separator) is escaped — JSON allows it, JS forbids
        // it inside string literals.
        assert_eq!(json_to_js_expression("\"\u{2028}foo\""), "\"\\u2028foo\"");
        // U+2029 (paragraph separator) is escaped for the same reason.
        assert_eq!(json_to_js_expression("\"bar\u{2029}\""), "\"bar\\u2029\"");
        // Normal JSON objects are valid JS expression syntax already.
        let input = r#"{"key":"value"}"#;
        assert_eq!(json_to_js_expression(input), input);
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
