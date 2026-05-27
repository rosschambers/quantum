use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use super::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::InfrastructureError;

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
    ) -> Result<(), InfrastructureError> {
        // Remove stale socket
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)
                .map_err(|e| InfrastructureError::Io(e.to_string()))?;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .map_err(|e| InfrastructureError::Io(e.to_string()))?;

        loop {
            let (stream, _) = listener
                .accept()
                .await
                .map_err(|e| InfrastructureError::Io(e.to_string()))?;

            let dispatcher = dispatcher.clone();
            tokio::spawn(async move {
                let _ = handle_connection(stream, dispatcher).await;
            });
        }
    }
}

/// Handle a single client connection.
async fn handle_connection<D: Dispatcher + 'static>(
    stream: UnixStream,
    dispatcher: Arc<D>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;

        if n == 0 {
            break; // Connection closed
        }

        // Parse JSON-RPC request
        match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => {
                let response = handle_request(&request, dispatcher.clone()).await;

                let response_json = serde_json::to_string(&response)?;
                writer.write_all(response_json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
            }
            Err(e) => {
                let error_response = JsonRpcResponse::error(
                    None,
                    JsonRpcError::new(-32700, format!("Parse error: {}", e)),
                );
                let response_json = serde_json::to_string(&error_response)?;
                writer.write_all(response_json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
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

        // Spawn server in background
        let socket_path_clone = socket_path.clone();
        tokio::spawn(async move {
            let _ = server.serve(dispatcher).await;
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
}
