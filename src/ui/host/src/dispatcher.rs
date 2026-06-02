//! IPC dispatcher trait for ui crate.
//! This trait is implemented by the quantumd binary to bridge between
//! the ui crate (which cannot depend on infrastructure) and the actual IPC dispatcher.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Error from dispatcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchError {
    pub code: i32,
    pub message: String,
}

/// Result type for dispatch operations.
pub type DispatchResult = Result<Value, DispatchError>;

/// Trait for dispatching JSON-RPC requests from the UI.
/// This trait is implemented in quantumd to forward to the actual dispatcher.
#[async_trait]
pub trait IpcDispatcher: Send + Sync {
    async fn dispatch(&self, method: &str, params: Value) -> DispatchResult;
}
