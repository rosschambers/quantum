use futures::stream::StreamExt;
use quantum_domain::{DomainError, ProviderId, ProviderRegistry};
use std::sync::Arc;
use std::time::Duration;

use crate::error::{ApplicationError, Result};

/// One-shot query for the current state of a streaming provider.
///
/// Prefers the provider's explicit `ProviderSource::snapshot()` when it
/// returns `Some(value)`. Otherwise falls back to `subscribe()`, taking the
/// first emission with a timeout and returning it as JSON. Each streaming
/// provider's first emission is its current state (battery percent, bluetooth
/// devices, `unavailable` snapshot, etc.) so this gives subscribers a way to
/// catch up without waiting for the next change event.
///
/// Returns `DomainError::Unsupported` if the provider exposes neither an
/// explicit snapshot nor a subscription, or if no event arrives within the
/// timeout window.
pub struct QueryProviderUseCase {
    registry: Arc<dyn ProviderRegistry>,
    timeout: Duration,
}

impl QueryProviderUseCase {
    pub fn new(registry: Arc<dyn ProviderRegistry>) -> Self {
        Self {
            registry,
            timeout: Duration::from_secs(2),
        }
    }

    #[cfg(test)]
    pub fn with_timeout(registry: Arc<dyn ProviderRegistry>, timeout: Duration) -> Self {
        Self { registry, timeout }
    }

    pub async fn execute(&self, provider_id: ProviderId) -> Result<serde_json::Value> {
        let provider = self.registry.get(&provider_id).await.ok_or_else(|| {
            ApplicationError::Domain(DomainError::ProviderNotFound(provider_id.clone()))
        })?;
        // Prefer the explicit one-shot snapshot when the provider supplies one;
        // it is a non-fragile path that does not rely on the streaming
        // "first emission is current state" invariant.
        if let Some(value) = provider.snapshot().await {
            return Ok(value);
        }
        let mut stream = provider.subscribe().ok_or_else(|| {
            ApplicationError::Domain(DomainError::Unsupported(format!(
                "provider {provider_id} does not support subscriptions"
            )))
        })?;
        match tokio::time::timeout(self.timeout, stream.next()).await {
            Ok(Some(payload)) => Ok(payload),
            Ok(None) => Err(ApplicationError::Domain(DomainError::Unsupported(format!(
                "provider {provider_id} subscription ended without yielding state"
            )))),
            Err(_) => Err(ApplicationError::Domain(DomainError::Unsupported(format!(
                "provider {provider_id} did not yield state within {:?}",
                self.timeout
            )))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream, StreamExt};
    use quantum_domain::{Action, ActionOutcome, Match, ProviderSource, Query};
    use std::sync::Mutex;

    struct YieldingProvider {
        id: ProviderId,
        value: serde_json::Value,
    }

    #[async_trait]
    impl ProviderSource for YieldingProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }
        async fn search(&self, _: &Query) -> std::result::Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }
        async fn invoke(&self, _: &Action) -> std::result::Result<ActionOutcome, DomainError> {
            Err(DomainError::Unsupported("test".into()))
        }
        fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
            let v = self.value.clone();
            let initial = stream::iter(std::iter::once(v));
            let pending: stream::Pending<serde_json::Value> = stream::pending();
            Some(Box::pin(initial.chain(pending)))
        }
    }

    struct SilentProvider {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for SilentProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }
        async fn search(&self, _: &Query) -> std::result::Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }
        async fn invoke(&self, _: &Action) -> std::result::Result<ActionOutcome, DomainError> {
            Err(DomainError::Unsupported("test".into()))
        }
        fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
            let pending: stream::Pending<serde_json::Value> = stream::pending();
            Some(Box::pin(pending))
        }
    }

    /// A provider whose explicit `snapshot()` returns a known value and whose
    /// `subscribe()` panics. If the use case ever falls through to the stream
    /// path the panic proves the snapshot was not preferred.
    struct SnapshotProvider {
        id: ProviderId,
        value: serde_json::Value,
    }

    #[async_trait]
    impl ProviderSource for SnapshotProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }
        async fn search(&self, _: &Query) -> std::result::Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }
        async fn invoke(&self, _: &Action) -> std::result::Result<ActionOutcome, DomainError> {
            Err(DomainError::Unsupported("test".into()))
        }
        fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
            panic!("snapshot() must be preferred; subscribe() must not be called");
        }
        async fn snapshot(&self) -> Option<serde_json::Value> {
            Some(self.value.clone())
        }
    }

    struct NonStreamingProvider {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for NonStreamingProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }
        async fn search(&self, _: &Query) -> std::result::Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }
        async fn invoke(&self, _: &Action) -> std::result::Result<ActionOutcome, DomainError> {
            Err(DomainError::Unsupported("test".into()))
        }
    }

    struct FakeRegistry {
        providers: Mutex<Vec<(ProviderId, Arc<dyn ProviderSource>)>>,
    }

    #[async_trait]
    impl ProviderRegistry for FakeRegistry {
        async fn list(&self) -> Vec<ProviderId> {
            self.providers
                .lock()
                .unwrap()
                .iter()
                .map(|(id, _)| id.clone())
                .collect()
        }
        async fn get(&self, id: &ProviderId) -> Option<Arc<dyn ProviderSource>> {
            self.providers
                .lock()
                .unwrap()
                .iter()
                .find(|(pid, _)| pid == id)
                .map(|(_, p)| p.clone())
        }
    }

    fn registry_with(provider: Arc<dyn ProviderSource>) -> Arc<dyn ProviderRegistry> {
        let id = provider.id().clone();
        Arc::new(FakeRegistry {
            providers: Mutex::new(vec![(id, provider)]),
        })
    }

    #[tokio::test]
    async fn returns_first_emission_from_streaming_provider() {
        let p = Arc::new(YieldingProvider {
            id: "test".into(),
            value: serde_json::json!({"available": true, "v": 42}),
        }) as Arc<dyn ProviderSource>;
        let uc = QueryProviderUseCase::new(registry_with(p));
        let v = uc.execute("test".into()).await.unwrap();
        assert_eq!(v["v"], 42);
    }

    #[tokio::test]
    async fn prefers_snapshot_over_subscribe_when_overridden() {
        let p = Arc::new(SnapshotProvider {
            id: "test".into(),
            value: serde_json::json!({"snapshot": true, "v": 7}),
        }) as Arc<dyn ProviderSource>;
        let uc = QueryProviderUseCase::new(registry_with(p));
        let v = uc.execute("test".into()).await.unwrap();
        assert_eq!(v["snapshot"], true);
        assert_eq!(v["v"], 7);
    }

    #[tokio::test]
    async fn falls_back_to_subscribe_when_snapshot_is_default_none() {
        // YieldingProvider does not override snapshot(), so the default None
        // forces the subscribe-first-emission fallback path.
        let p = Arc::new(YieldingProvider {
            id: "test".into(),
            value: serde_json::json!({"available": true, "v": 99}),
        }) as Arc<dyn ProviderSource>;
        let uc = QueryProviderUseCase::new(registry_with(p));
        let v = uc.execute("test".into()).await.unwrap();
        assert_eq!(v["v"], 99);
    }

    #[tokio::test]
    async fn returns_error_when_provider_does_not_support_subscriptions() {
        let p = Arc::new(NonStreamingProvider { id: "test".into() }) as Arc<dyn ProviderSource>;
        let uc = QueryProviderUseCase::new(registry_with(p));
        let r = uc.execute("test".into()).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn returns_error_when_provider_does_not_yield_within_timeout() {
        let p = Arc::new(SilentProvider { id: "test".into() }) as Arc<dyn ProviderSource>;
        let uc = QueryProviderUseCase::with_timeout(registry_with(p), Duration::from_millis(50));
        let r = uc.execute("test".into()).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn returns_provider_not_found_when_id_unknown() {
        let p = Arc::new(YieldingProvider {
            id: "test".into(),
            value: serde_json::json!({}),
        }) as Arc<dyn ProviderSource>;
        let uc = QueryProviderUseCase::new(registry_with(p));
        let r = uc.execute("nope".into()).await;
        assert!(matches!(
            r,
            Err(ApplicationError::Domain(DomainError::ProviderNotFound(_)))
        ));
    }
}
