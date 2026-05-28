//! Cross-layer event envelope.
//!
//! `EventEnvelope` is the wire-level shape of a broadcast event. It lives in
//! the domain crate because both the infrastructure (IPC socket clients) and
//! the UI (WebView `__quantum_notify` bridge) need to consume it without the
//! UI layer being allowed to depend on infrastructure.

use serde::{Deserialize, Serialize};

/// An event envelope broadcast from the domain `EventBus` to listeners
/// (IPC socket clients and WebView frontends).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub channel: String,
    pub payload: serde_json::Value,
}
