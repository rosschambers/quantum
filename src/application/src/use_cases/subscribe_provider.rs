use futures::stream::StreamExt;
use quantum_domain::{DomainError, EventBus, ProviderId, ProviderRegistry};
use std::sync::Arc;

use crate::error::{ApplicationError, Result};

pub struct SubscribeProviderUseCase {
    registry: Arc<dyn ProviderRegistry>,
    event_bus: Arc<dyn EventBus>,
}

impl SubscribeProviderUseCase {
    pub fn new(registry: Arc<dyn ProviderRegistry>, event_bus: Arc<dyn EventBus>) -> Self {
        Self {
            registry,
            event_bus,
        }
    }

    pub async fn execute(&self, provider_id: ProviderId) -> Result<()> {
        let provider = self.registry.get(&provider_id).await.ok_or_else(|| {
            ApplicationError::Domain(DomainError::ProviderNotFound(provider_id.clone()))
        })?;
        let stream = provider.subscribe().ok_or_else(|| {
            ApplicationError::Domain(DomainError::Unsupported(format!(
                "provider {provider_id} does not support subscriptions"
            )))
        })?;
        let event_bus = self.event_bus.clone();
        let channel = format!("{provider_id}.event");
        tokio::spawn(async move {
            let mut stream = stream;
            while let Some(payload) = stream.next().await {
                let payload_str = payload.to_string();
                let _ = event_bus.publish(&channel, &payload_str).await;
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream::{self, StreamExt};
    use std::sync::Mutex;
    use tokio::sync::Mutex as TokioMutex;

    struct FakeProvider {
        id: ProviderId,
    }

    #[async_trait]
    impl quantum_domain::ProviderSource for FakeProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }
        async fn search(
            &self,
            _: &quantum_domain::Query,
        ) -> std::result::Result<Vec<quantum_domain::Match>, DomainError> {
            Ok(vec![])
        }
        async fn invoke(
            &self,
            _: &quantum_domain::Action,
        ) -> std::result::Result<quantum_domain::ActionOutcome, DomainError> {
            Err(DomainError::Unsupported("fake provider".into()))
        }
        fn subscribe(&self) -> Option<futures::stream::BoxStream<'static, serde_json::Value>> {
            let events = vec![serde_json::json!({"x": 1}), serde_json::json!({"x": 2})];
            Some(stream::iter(events).boxed())
        }
    }

    struct FakeRegistry {
        providers: Mutex<Vec<(ProviderId, Arc<dyn quantum_domain::ProviderSource>)>>,
    }

    #[async_trait]
    impl quantum_domain::ProviderRegistry for FakeRegistry {
        async fn list(&self) -> Vec<ProviderId> {
            self.providers
                .lock()
                .unwrap()
                .iter()
                .map(|(id, _)| id.clone())
                .collect()
        }
        async fn get(&self, id: &ProviderId) -> Option<Arc<dyn quantum_domain::ProviderSource>> {
            self.providers
                .lock()
                .unwrap()
                .iter()
                .find(|(pid, _)| pid == id)
                .map(|(_, p)| p.clone())
        }
    }

    struct FakeEventBus {
        events: TokioMutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl EventBus for FakeEventBus {
        async fn publish(
            &self,
            event: &str,
            payload: &str,
        ) -> std::result::Result<(), DomainError> {
            self.events
                .lock()
                .await
                .push((event.to_string(), payload.to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn forwards_stream_events_through_event_bus() {
        let provider = Arc::new(FakeProvider { id: "test".into() });
        let registry = FakeRegistry {
            providers: Mutex::new(vec![(
                "test".into(),
                provider as Arc<dyn quantum_domain::ProviderSource>,
            )]),
        };
        let bus = Arc::new(FakeEventBus {
            events: TokioMutex::new(Vec::new()),
        });
        let uc = SubscribeProviderUseCase::new(Arc::new(registry), bus.clone());
        uc.execute("test".into()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let events = bus.events.lock().await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "test.event");
        assert!(events[0].1.contains("\"x\":1"));
    }
}
