use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

use crate::error::IpcError;
use crate::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use quantum_domain::EventEnvelope;

/// Maximum size in bytes of a single JSON-RPC request line. A misbehaving
/// client that never sends a newline would otherwise grow the read buffer
/// without bound. Real requests are tiny; 1 MiB is generous headroom for
/// theme.reload-style payloads.
const MAX_LINE_BYTES: u64 = 1024 * 1024;

/// Result type for dispatch operations.
pub type DispatchResult = Result<Value, DispatchError>;

/// Error from dispatcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchError {
    pub code: i32,
    pub message: String,
}

impl DispatchError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Trait for dispatching JSON-RPC requests.
#[async_trait]
pub trait Dispatcher: Send + Sync {
    async fn dispatch(&self, method: &str, params: Value) -> DispatchResult;
}

/// Unix socket IPC server.
pub struct UnixSocketServer {
    socket_path: std::path::PathBuf,
}

impl UnixSocketServer {
    /// Create a new IPC server.
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    /// Start the server and accept connections.
    pub async fn serve<D: Dispatcher + 'static>(
        &self,
        dispatcher: Arc<D>,
        broadcast_tx: broadcast::Sender<EventEnvelope>,
    ) -> Result<(), IpcError> {
        // Remove stale socket
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path).map_err(|e| IpcError::Io(e.to_string()))?;
        }

        let listener =
            UnixListener::bind(&self.socket_path).map_err(|e| IpcError::Io(e.to_string()))?;

        loop {
            let (stream, _) = listener
                .accept()
                .await
                .map_err(|e| IpcError::Io(e.to_string()))?;

            let dispatcher = dispatcher.clone();
            let broadcast_tx = broadcast_tx.clone();
            tokio::spawn(async move {
                let _ = handle_connection(stream, dispatcher, broadcast_tx).await;
            });
        }
    }
}

/// Handle a single client connection.
///
/// One task owns both halves of the connection: it reads JSON-RPC requests
/// from the client and forwards broadcast events as JSON-RPC notifications
/// to the same client. Using `tokio::select!` over both sources means that
/// when the client disconnects (read returns EOF or error), the broadcast
/// forwarder exits with it — no orphaned task, no leaked receiver slot,
/// no need to share the writer behind a mutex.
async fn handle_connection<D: Dispatcher + 'static>(
    stream: UnixStream,
    dispatcher: Arc<D>,
    broadcast_tx: broadcast::Sender<EventEnvelope>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (reader, writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    // Wrap the writer in a BufWriter so body+newline writes coalesce
    // into a single syscall per frame (flushed explicitly below). 8 KiB
    // matches BufReader's default and is plenty for typical JSON-RPC
    // payloads. Without the per-frame flush the kernel would not see
    // bytes until the buffer filled, which would stall responses.
    let mut writer = BufWriter::new(writer);
    let mut event_rx = broadcast_tx.subscribe();
    let mut line = String::new();

    loop {
        line.clear();
        // `take` is reconstructed per iteration so its remaining-byte
        // counter resets — we want a per-line cap, not a per-connection
        // cap. Binding to a `let` keeps the temporary alive for the full
        // duration of the `select!` future.
        let mut limited_reader = (&mut reader).take(MAX_LINE_BYTES);
        tokio::select! {
            // Read JSON-RPC requests from the client. The `take` adapter
            // caps the bytes a single `read_line` call will consume so a
            // client that never sends a newline cannot grow `line`
            // without bound and OOM the daemon.
            result = limited_reader.read_line(&mut line) => {
                match result {
                    Ok(0) => break, // Connection closed
                    Ok(n) if (n as u64) >= MAX_LINE_BYTES => {
                        tracing::warn!(
                            "ipc: request exceeded {} bytes without newline; closing connection",
                            MAX_LINE_BYTES
                        );
                        let error_response = JsonRpcResponse::error(
                            None,
                            JsonRpcError::new(-32700, "request too large"),
                        );
                        if let Ok(response_json) = serde_json::to_string(&error_response) {
                            let _ = writer.write_all(response_json.as_bytes()).await;
                            let _ = writer.write_all(b"\n").await;
                            let _ = writer.flush().await;
                        }
                        break;
                    }
                    Ok(_) => {
                        // Parse JSON-RPC request
                        match serde_json::from_str::<JsonRpcRequest>(&line) {
                            Ok(request) => {
                                let response = handle_request(&request, dispatcher.clone()).await;
                                let response_json = serde_json::to_string(&response)?;
                                writer.write_all(response_json.as_bytes()).await?;
                                writer.write_all(b"\n").await?;
                                writer.flush().await?;
                            }
                            Err(e) => {
                                let error_response = JsonRpcResponse::error(
                                    None,
                                    JsonRpcError::new(-32700, format!("Parse error: {}", e)),
                                );
                                let response_json = serde_json::to_string(&error_response)?;
                                writer.write_all(response_json.as_bytes()).await?;
                                writer.write_all(b"\n").await?;
                                writer.flush().await?;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("ipc: read error: {e}");
                        break;
                    }
                }
            }
            // Forward broadcast events as JSON-RPC notifications.
            event = event_rx.recv() => {
                match event {
                    Ok(env) => {
                        // `env.payload` is `Box<RawValue>` carrying raw JSON text
                        // straight from the publisher. We inline it verbatim into
                        // the JSON-RPC notification — no `to_value` round trip,
                        // and the channel name still needs proper escaping so we
                        // serialize that one field with `serde_json::to_string`.
                        let channel_json = match serde_json::to_string(&env.channel) {
                            Ok(s) => s,
                            Err(err) => {
                                tracing::warn!("ipc: failed to serialize channel: {err}");
                                continue;
                            }
                        };
                        let notification = format!(
                            "{{\"jsonrpc\":\"2.0\",\"method\":{},\"params\":{}}}\n",
                            channel_json,
                            env.payload.get()
                        );
                        if writer.write_all(notification.as_bytes()).await.is_err() {
                            break;
                        }
                        if writer.flush().await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    Ok(())
}

/// Handle a single JSON-RPC request.
async fn handle_request<D: Dispatcher>(
    request: &JsonRpcRequest,
    dispatcher: Arc<D>,
) -> JsonRpcResponse {
    let params = request
        .params
        .clone()
        .unwrap_or(Value::Object(Default::default()));

    match dispatcher.dispatch(&request.method, params).await {
        Ok(result) => JsonRpcResponse::success(request.id.clone(), result),
        Err(e) => {
            let error = JsonRpcError::new(e.code, e.message.clone())
                .with_data(json!({ "error": e.message }));

            JsonRpcResponse::error(request.id.clone(), error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDispatcher;

    #[async_trait]
    impl Dispatcher for FakeDispatcher {
        async fn dispatch(&self, method: &str, _params: Value) -> DispatchResult {
            match method {
                "test.ping" => Ok(json!({"pong": true})),
                "test.error" => Err(DispatchError::new(-32603, "test error")),
                _ => Err(DispatchError::new(-32603, "unknown method")),
            }
        }
    }

    #[tokio::test]
    async fn handle_request_success() {
        let dispatcher = Arc::new(FakeDispatcher);
        let request = JsonRpcRequest::new("test.ping", 1);

        let response = handle_request(&request, dispatcher).await;

        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn handle_request_error() {
        let dispatcher = Arc::new(FakeDispatcher);
        let request = JsonRpcRequest::new("test.error", 2);

        let response = handle_request(&request, dispatcher).await;

        assert!(response.result.is_none());
        assert!(response.error.is_some());
    }

    #[tokio::test]
    async fn server_accepts_connections() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server = UnixSocketServer::new(&socket_path);
        let dispatcher = Arc::new(FakeDispatcher);
        let (broadcast_tx, _) = broadcast::channel::<EventEnvelope>(16);

        // Spawn server in background
        let socket_path_clone = socket_path.clone();
        tokio::spawn(async move {
            let _ = server.serve(dispatcher, broadcast_tx).await;
        });

        // Give server time to start
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Connect and send request
        if let Ok(mut stream) = UnixStream::connect(&socket_path_clone).await {
            let request = JsonRpcRequest::new("test.ping", 1);
            let request_json = serde_json::to_string(&request).unwrap();

            let _ = stream.write_all(request_json.as_bytes()).await;
            let _ = stream.write_all(b"\n").await;

            let mut reader = BufReader::new(tokio::io::split(stream).0);
            let mut response = String::new();
            let _ = reader.read_line(&mut response).await;

            assert!(!response.is_empty());
        }
    }

    #[tokio::test]
    async fn server_forwards_broadcast_events_as_notifications() {
        use std::time::Duration;

        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server = Arc::new(UnixSocketServer::new(&socket_path));
        let dispatcher = Arc::new(FakeDispatcher);
        let (broadcast_tx, _) = broadcast::channel::<EventEnvelope>(16);

        // Spawn server in background
        let server_for_task = server.clone();
        let tx_for_task = broadcast_tx.clone();
        tokio::spawn(async move {
            let _ = server_for_task.serve(dispatcher, tx_for_task).await;
        });

        // Wait for socket to exist
        for _ in 0..40 {
            if socket_path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        // Connect to server
        let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let (read, _write) = stream.into_split();
        let mut reader = BufReader::new(read);

        // Give the connection a moment to subscribe to events
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Send an event through the broadcast channel
        let payload =
            serde_json::value::RawValue::from_string(json!({"css": ":root {}"}).to_string())
                .expect("valid JSON");
        broadcast_tx
            .send(EventEnvelope {
                channel: "theme.reloaded".to_string(),
                payload,
            })
            .ok();

        // Read the notification
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(1), reader.read_line(&mut line))
            .await
            .expect("notification arrives within 1s")
            .unwrap();

        // Parse and verify the notification
        let parsed: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "theme.reloaded");
        assert_eq!(parsed["params"]["css"], ":root {}");
    }
}
