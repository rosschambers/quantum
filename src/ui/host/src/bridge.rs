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
pub fn json_to_js_expression(json: &str) -> String {
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
        // `msg` is a `javascriptcore::Value` from the JS side. We need to
        // pull a JS object out of it as a serde_json::Value, regardless of
        // whether the caller passed an object directly or a JSON-encoded
        // string. The TS client today calls `postMessage(JSON.stringify(req))`
        // which arrives here as a JS string — `to_json(0)` on a JS string
        // produces a quoted, escape-encoded JSON string literal. Parse that
        // out first, then if the result is itself a JSON string, parse that
        // inner string. This handles both shapes transparently.
        let outer_json = match msg.to_json(0) {
            Some(s) => s.to_string(),
            None => {
                tracing::warn!("script message could not be serialized to JSON");
                return;
            }
        };

        let payload_value: Value = match serde_json::from_str::<Value>(&outer_json) {
            Ok(Value::String(inner)) => match serde_json::from_str::<Value>(&inner) {
                Ok(v) => v,
                Err(err) => {
                    tracing::warn!(
                        "bridge message wrapped a string that wasn't JSON: {err} (raw: {inner})"
                    );
                    return;
                }
            },
            Ok(v) => v,
            Err(err) => {
                tracing::warn!("bridge message wasn't valid JSON: {err} (raw: {outer_json})");
                return;
            }
        };

        let Ok(parsed): Result<BridgeMessage, _> = serde_json::from_value(payload_value) else {
            tracing::warn!("bridge message did not match BridgeMessage shape: {outer_json}");
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
