//! Cross-layer event envelope.
//!
//! `EventEnvelope` is the wire-level shape of a broadcast event. It lives in
//! the domain crate because both the infrastructure (IPC socket clients) and
//! the UI (WebView `__quantum_notify` bridge) need to consume it without the
//! UI layer being allowed to depend on infrastructure.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// An event envelope broadcast from the domain `EventBus` to listeners
/// (IPC socket clients and WebView frontends).
///
/// `payload` is `Box<RawValue>` rather than `serde_json::Value` so the JSON
/// string from the original publisher (`EventBus::publish(channel, payload)`)
/// can travel from publisher to every subscriber without an intermediate
/// `from_str` -> `to_string` round-trip. Subscribers that need a typed view
/// call `serde_json::from_str` on `payload.get()` themselves; subscribers
/// that re-emit JSON (IPC server, WebView bridge) take `payload.get()` and
/// inline it verbatim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub channel: String,
    pub payload: Box<RawValue>,
}
