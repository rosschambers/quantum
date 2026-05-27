use crate::{ApplicationError, Result};
use quantum_domain::{Action, ProviderId, ProviderRegistry};
use std::sync::Arc;

pub struct LaunchActionUseCase {
    registry: Arc<dyn ProviderRegistry>,
}

impl LaunchActionUseCase {
    pub fn new(registry: Arc<dyn ProviderRegistry>) -> Self {
        Self { registry }
    }

    pub async fn execute(&self, provider_id: ProviderId, action: Action) -> Result<()> {
        let provider = self
            .registry
            .get(&provider_id)
            .await
            .ok_or_else(|| {
                ApplicationError::Domain(quantum_domain::DomainError::ProviderNotFound(
                    provider_id.clone(),
                ))
            })?;

        provider.invoke(&action).await.map_err(|source| {
            ApplicationError::Dispatch {
                method: "action.invoke".to_string(),
                source,
            }
        })?;

        Ok(())
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
        should_fail: bool,
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

        async fn invoke(&self, _a: &Action) -> std::result::Result<ActionOutcome, DomainError> {
            if self.should_fail {
                Err(DomainError::ActionFailed {
                    reason: "simulated failure".to_string(),
                })
            } else {
                Ok(ActionOutcome { message: None })
            }
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

        fn with_provider(mut self, id: ProviderId, should_fail: bool) -> Self {
            let provider = FakeProvider { id: id.clone(), should_fail };
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
    async fn launches_action_successfully() {
        let reg = FakeRegistry::new().with_provider("apps".into(), false);
        let uc = LaunchActionUseCase::new(Arc::new(reg));

        let action = Action::Launch {
            desktop_id: "firefox".to_string(),
        };

        let result = uc.execute("apps".into(), action).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn returns_error_for_missing_provider() {
        let reg = FakeRegistry::new();
        let uc = LaunchActionUseCase::new(Arc::new(reg));

        let action = Action::Launch {
            desktop_id: "firefox".to_string(),
        };

        let result = uc.execute("apps".into(), action).await;
        assert!(matches!(result, Err(ApplicationError::Domain(quantum_domain::DomainError::ProviderNotFound(_)))));
    }

    #[tokio::test]
    async fn returns_dispatch_error_on_provider_failure() {
        let reg = FakeRegistry::new().with_provider("apps".into(), true);
        let uc = LaunchActionUseCase::new(Arc::new(reg));

        let action = Action::Launch {
            desktop_id: "firefox".to_string(),
        };

        let result = uc.execute("apps".into(), action).await;
        assert!(matches!(result, Err(ApplicationError::Dispatch { .. })));
    }
}
