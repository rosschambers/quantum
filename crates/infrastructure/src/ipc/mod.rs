pub mod protocol;
pub mod server;

pub use protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
pub use server::UnixSocketServer;

/// Re-export of the domain `EventEnvelope` so existing consumers of
/// `quantum_infrastructure::ipc::EventEnvelope` keep compiling. The type now
/// lives in `quantum_domain` so both the UI (WebView bridge) and the IPC
/// socket server can share it without the UI layer depending on
/// infrastructure.
pub use quantum_domain::EventEnvelope;
