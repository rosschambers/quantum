//! WebKit script message bridge to Dispatcher.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::runtime::Handle;
use webkit6::{prelude::*, WebView};

use crate::dispatcher::{DispatchError, DispatchResult};
use crate::subscriptions::WebviewSubscriptions;
use crate::viewer_image_resources::{bind_view, prepare_image, PreparedImage};
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

struct ResponseExpressions {
    completed: String,
    cancelled: String,
    #[cfg(test)]
    serialization_thread: std::thread::ThreadId,
}

async fn serialize_response(
    runtime: &Handle,
    identity: u64,
    result: DispatchResult,
) -> ResponseExpressions {
    let serialization = runtime.spawn_blocking(move || {
        let completed = match result {
            Ok(value) => {
                let payload = serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
                let payload = json_to_js_expression(&payload);
                format!("window.__quantum_resolve({identity}, {payload})")
            }
            Err(error) => {
                let payload = serde_json::to_string(&error).unwrap_or_else(|_| "{}".into());
                let payload = json_to_js_expression(&payload);
                format!("window.__quantum_reject({identity}, {payload})")
            }
        };
        ResponseExpressions {
            completed,
            cancelled: format!(
                "window.__quantum_reject({identity}, {{\"code\":-32800,\"message\":\"read cancelled because viewer state changed\"}})"
            ),
            #[cfg(test)]
            serialization_thread: std::thread::current().id(),
        }
    });
    serialization.await.unwrap_or_else(|error| {
        tracing::error!(%error, "bridge response serialization worker failed");
        // Only this fixed, small emergency response is constructed on GLib.
        // Arbitrarily large dispatcher success/error payloads never are.
        let completed = format!("window.__quantum_reject({identity}, {{\"code\":-32603,\"message\":\"response serialization failed\"}})");
        ResponseExpressions {
            cancelled: completed.clone(),
            completed,
            #[cfg(test)]
            serialization_thread: std::thread::current().id(),
        }
    })
}

/// Register the bridge message handler on a WebView.
/// Wires WebKit script messages to the Tokio dispatcher with JS evaluation for responses.
///
/// `subs` is the webview's per-webview subscription set. The two webview-local
/// methods `bridge.subscribe` / `bridge.unsubscribe` mutate it here instead of
/// dispatching to the global dispatcher, so the paired forwarder can filter the
/// broadcast down to the channels this webview actually asked for.
pub fn register_bridge(
    webview: &WebView,
    dispatcher: Arc<dyn IpcDispatcher>,
    runtime: Handle,
    subs: WebviewSubscriptions,
) {
    let ucm = match webview.user_content_manager() {
        Some(mgr) => mgr,
        None => {
            tracing::error!("failed to get user content manager");
            return;
        }
    };

    ucm.register_script_message_handler("quantum", None);

    let webview_weak = webview.downgrade();
    let resources = bind_view(webview);

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

        let webview = webview_weak.clone();
        let id = parsed.id;
        let is_viewer_read = parsed.method == "file-viewer.read";
        let document_generation = resources.document_generation();
        let generation = if is_viewer_read {
            resources.begin_read()
        } else {
            None
        };
        let resources = resources.clone();

        // One-shot channel for the dispatch result and worker-opened descriptor.
        // GTK ownership is bound only after this reaches the GLib thread.
        // The GLib receiver yields until the dispatcher and file open complete.
        let (tx, rx) = tokio::sync::oneshot::channel::<(DispatchResult, Option<PreparedImage>)>();

        if is_viewer_read && generation.is_none() {
            let _ = tx.send((
                Err(DispatchError {
                    code: -32800,
                    message: "read cancelled because viewer state changed".to_string(),
                }),
                None,
            ));
        } else if parsed.method == "bridge.subscribe" || parsed.method == "bridge.unsubscribe" {
            // Webview-local: update this webview's subscription set and resolve
            // the caller's promise WITHOUT dispatching to the global
            // dispatcher. Once seeded the set stays `Some`, so an unsubscribe
            // of the last channel narrows the filter to nothing but never
            // reverts to the forward-all state.
            if let Some(channel) = parsed.params.get("channel").and_then(Value::as_str) {
                if let Ok(mut guard) = subs.lock() {
                    if parsed.method == "bridge.subscribe" {
                        guard
                            .get_or_insert_with(HashSet::new)
                            .insert(channel.to_string());
                    } else if let Some(set) = guard.as_mut() {
                        set.remove(channel);
                    }
                }
            }
            // Resolve so the client's `call` settles. Reuse the same
            // oneshot -> spawn_local evaluation path the dispatched case uses.
            let _ = tx.send((Ok(Value::Null), None));
        } else {
            let dispatcher = dispatcher.clone();
            let method = parsed.method.clone();
            let params = parsed.params.clone();

            runtime.spawn(async move {
                let mut result = dispatcher.dispatch(&method, params).await;
                let mut image = None;
                if is_viewer_read {
                    if let Ok(value) = &result {
                        let value = value.clone();
                        match tokio::task::spawn_blocking(move || prepare_image(&value)).await {
                            Ok(Ok(prepared)) => image = prepared,
                            _ => {
                                result = Err(DispatchError {
                                    code: -32000,
                                    message: "image resource unavailable".to_string(),
                                })
                            }
                        }
                    }
                }
                // Ignore send failure: the GTK side has gone away.
                let _ = tx.send((result, image));
            });
        }

        // GTK objects and the grant store remain on their owning GLib thread.
        let response_runtime = runtime.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok((mut result, mut image)) = rx.await {
                if !resources.is_document_current(&document_generation) {
                    return;
                }
                if generation
                    .as_ref()
                    .is_some_and(|generation| !resources.is_current(generation))
                {
                    result = Err(DispatchError {
                        code: -32800,
                        message: "read cancelled because viewer state changed".to_string(),
                    });
                    image = None;
                }
                if let (Ok(value), Some(image)) = (&mut result, image) {
                    if let Some(uri) = generation
                        .as_ref()
                        .and_then(|generation| resources.issue(generation, image))
                    {
                        value["uri"] = Value::String(uri);
                    } else {
                        result = Err(DispatchError {
                            code: -32000,
                            message: "image resource unavailable".to_string(),
                        });
                    }
                }
                let expressions = serialize_response(&response_runtime, id, result).await;
                if !resources.is_document_current(&document_generation) {
                    return;
                }
                let Some(webview) = webview.upgrade() else {
                    return;
                };
                let expression = if generation
                    .as_ref()
                    .is_some_and(|generation| !resources.is_current(generation))
                {
                    &expressions.cancelled
                } else {
                    &expressions.completed
                };
                webview.evaluate_javascript(
                    expression,
                    None,
                    None,
                    None::<&gio::Cancellable>,
                    |_| {},
                );
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn response_serialization_runs_off_the_glib_owner_thread() {
        let context = glib::MainContext::new();
        let owner = std::thread::current().id();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .build()
            .unwrap();
        let large_text = format!("{}\u{2028}\u{2029}", "x".repeat(2 * 1024 * 1024));
        for result in [
            Ok(json!({"content": large_text})),
            Err(DispatchError {
                code: -32000,
                message: large_text.clone(),
            }),
        ] {
            let expressions = context.block_on(async {
                assert!(context.is_owner());
                serialize_response(runtime.handle(), 42, result).await
            });
            assert_ne!(
                expressions.serialization_thread, owner,
                "large success and error serialization must not run on GLib"
            );
            assert!(expressions.completed.contains("\\u2028\\u2029"));
            assert!(!expressions.completed.contains('\u{2028}'));
            assert!(expressions.cancelled.contains("-32800"));
        }
    }

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
