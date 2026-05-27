use quantum_domain::{ProviderId, ProviderRegistry};
use std::sync::Arc;

pub struct ListProvidersUseCase {
    registry: Arc<dyn ProviderRegistry>,
}

impl ListProvidersUseCase {
    pub fn new(registry: Arc<dyn ProviderRegistry>) -> Self {
        Self { registry }
    }

    pub async fn execute(&self) -> Vec<ProviderId> {
        self.registry.list().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::{
        ActionOutcome, ProviderCapabilities, ProviderSource, Query, DomainError, Match,
    };
    use std::collections::HashMap;

    struct FakeProvider {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for FakeProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                searchable: true,
                streamable: false,
            }
        }

        async fn search(&self, _q: &Query) -> std::result::Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }

        async fn invoke(
            &self,
            _a: &quantum_domain::Action,
        ) -> std::result::Result<ActionOutcome, DomainError> {
            Ok(ActionOutcome { message: None })
        }
    }

    struct FakeRegistry {
        providers: HashMap<ProviderId, Arc<dyn ProviderSource>>,
    }

    impl FakeRegistry {
        fn new() -> Self {
            Self {
                providers: HashMap::new(),
            }
        }

        fn with_provider(mut self, id: ProviderId) -> Self {
            let provider = FakeProvider { id: id.clone() };
            self.providers.insert(id, Arc::new(provider));
            self
        }
    }

    #[async_trait]
    impl ProviderRegistry for FakeRegistry {
        async fn list(&self) -> Vec<ProviderId> {
            self.providers.keys().cloned().collect()
        }

        async fn get(&self, id: &ProviderId) -> Option<Arc<dyn ProviderSource>> {
            self.providers.get(id).cloned()
        }
    }

    #[tokio::test]
    async fn lists_all_providers() {
        let reg = FakeRegistry::new()
            .with_provider("apps".into())
            .with_provider("shell".into())
            .with_provider("hyprland".into());

        let uc = ListProvidersUseCase::new(Arc::new(reg));
        let providers = uc.execute().await;

        assert_eq!(providers.len(), 3);
        assert!(providers.contains(&ProviderId::from("apps")));
        assert!(providers.contains(&ProviderId::from("shell")));
        assert!(providers.contains(&ProviderId::from("hyprland")));
    }

    #[tokio::test]
    async fn returns_empty_list_when_no_providers() {
        let reg = FakeRegistry::new();
        let uc = ListProvidersUseCase::new(Arc::new(reg));
        let providers = uc.execute().await;

        assert!(providers.is_empty());
    }
}
