pub mod protocol;
pub mod server;

use serde::{Deserialize, Serialize};

pub use protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
pub use server::UnixSocketServer;

/// An event envelope broadcast from the domain EventBus to IPC clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub channel: String,
    pub payload: serde_json::Value,
}
