//! quantum-ipc: unix-socket IPC server + JSON-RPC protocol types.
//!
//! Re-exports `EventEnvelope` from quantum-domain so callers can keep
//! a single import root for IPC types.

pub mod error;
pub mod protocol;
pub mod server;

pub use error::IpcError;
pub use protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
pub use quantum_domain::EventEnvelope;
pub use server::{DispatchError, DispatchResult, Dispatcher, UnixSocketServer};
