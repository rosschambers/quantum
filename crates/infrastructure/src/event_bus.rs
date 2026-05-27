use async_trait::async_trait;

use quantum_domain::{DomainError, EventBus};

/// An in-memory event bus that logs published events via `tracing`.
///
/// The daemon does not yet have subscribers wired up; this implementation
/// satisfies the [`EventBus`] port so use cases can publish events without
/// the daemon failing to assemble.
pub struct InMemoryEventBus;

impl InMemoryEventBus {
    pub fn new() -> Self {
        Self
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn publish(&self, event: &str, payload: &str) -> Result<(), DomainError> {
        tracing::debug!(event, payload, "event published");
        Ok(())
    }

    async fn subscribe(&self, event: &str) -> Result<(), DomainError> {
        tracing::debug!(event, "subscribe requested");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_succeeds() {
        let bus = InMemoryEventBus::new();
        assert!(bus.publish("test", "{}").await.is_ok());
    }

    #[tokio::test]
    async fn subscribe_succeeds() {
        let bus = InMemoryEventBus::new();
        assert!(bus.subscribe("test").await.is_ok());
    }
}
