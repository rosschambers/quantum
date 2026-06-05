use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use quantum_domain::{ProviderId, ProviderRegistry, ProviderSource};

/// In-memory implementation of ProviderRegistry with interior mutability via RwLock.
pub struct InMemoryProviderRegistry {
    providers: RwLock<HashMap<ProviderId, Arc<dyn ProviderSource>>>,
}

impl InMemoryProviderRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a provider.
    pub async fn register(&self, id: ProviderId, provider: Arc<dyn ProviderSource>) {
        let mut providers = self.providers.write().await;
        providers.insert(id, provider);
    }
}

impl Default for InMemoryProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderRegistry for InMemoryProviderRegistry {
    async fn list(&self) -> Vec<ProviderId> {
        let providers = self.providers.read().await;
        providers.keys().cloned().collect()
    }

    async fn get(&self, id: &ProviderId) -> Option<Arc<dyn ProviderSource>> {
        let providers = self.providers.read().await;
        providers.get(id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::{Action, ActionOutcome, DomainError, Match, Query};

    struct TestProvider {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for TestProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }

        async fn search(&self, _q: &Query) -> Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }

        async fn invoke(&self, _action: &Action) -> Result<ActionOutcome, DomainError> {
            Ok(ActionOutcome { message: None })
        }
    }

    #[tokio::test]
    async fn register_and_list() {
        let registry = InMemoryProviderRegistry::new();
        let p1 = Arc::new(TestProvider {
            id: ProviderId::from("apps"),
        });
        let p2 = Arc::new(TestProvider {
            id: ProviderId::from("hypr"),
        });

        registry
            .register(ProviderId::from("apps"), p1.clone())
            .await;
        registry
            .register(ProviderId::from("hypr"), p2.clone())
            .await;

        let list = registry.list().await;
        assert_eq!(list.len(), 2);
        assert!(list.contains(&ProviderId::from("apps")));
        assert!(list.contains(&ProviderId::from("hypr")));
    }

    #[tokio::test]
    async fn get_returns_registered_provider() {
        let registry = InMemoryProviderRegistry::new();
        let p = Arc::new(TestProvider {
            id: ProviderId::from("apps"),
        });

        registry.register(ProviderId::from("apps"), p).await;

        let retrieved = registry.get(&ProviderId::from("apps")).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id(), &ProviderId::from("apps"));
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_provider() {
        let registry = InMemoryProviderRegistry::new();
        let retrieved = registry.get(&ProviderId::from("nonexistent")).await;
        assert!(retrieved.is_none());
    }
}
