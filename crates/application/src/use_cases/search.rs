use crate::Result;
use futures::stream::{FuturesUnordered, StreamExt};
use quantum_domain::{Match, ProviderRegistry, Query};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub matches: Vec<Match>,
    pub warnings: Vec<String>,
}

pub struct SearchUseCase {
    registry: Arc<dyn ProviderRegistry>,
}

impl SearchUseCase {
    pub fn new(registry: Arc<dyn ProviderRegistry>) -> Self {
        Self { registry }
    }

    pub async fn execute(&self, query: Query) -> Result<SearchResponse> {
        let provider_ids = if query.providers.is_empty() {
            self.registry.list().await
        } else {
            query.providers.clone()
        };

        let mut futures = FuturesUnordered::new();
        for provider_id in &provider_ids {
            let reg = self.registry.clone();
            let pid = provider_id.clone();
            let q = query.clone();
            futures.push(async move {
                match reg.get(&pid).await {
                    Some(provider) => {
                        match timeout(Duration::from_secs(2), provider.search(&q)).await {
                            Ok(Ok(matches)) => (Some(matches), None),
                            Ok(Err(e)) => (None, Some(format!("Provider {} error: {}", pid, e))),
                            Err(_) => (
                                None,
                                Some(format!("Provider {} timed out after 2 seconds", pid)),
                            ),
                        }
                    }
                    None => (None, Some(format!("Provider {} not found", pid))),
                }
            });
        }

        let mut all_matches = Vec::new();
        let mut warnings = Vec::new();

        while let Some((matches, warning)) = futures.next().await {
            if let Some(w) = warning {
                warnings.push(w);
            }
            if let Some(mut m) = matches {
                all_matches.append(&mut m);
            }
        }

        // Sort by score descending
        all_matches.sort_by(|a, b| {
            b.score
                .value()
                .partial_cmp(&a.score.value())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply limit
        if let Some(limit) = query.limit {
            all_matches.truncate(limit as usize);
        }

        Ok(SearchResponse {
            matches: all_matches,
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::{
        Action, DomainError, MatchScore, ProviderCapabilities, ProviderId, ProviderSource,
    };
    use std::collections::HashMap;
    use std::time::Duration as StdDuration;
    use tokio::time::sleep;

    struct FakeProvider {
        id: ProviderId,
        matches: Vec<Match>,
        delay: Option<StdDuration>,
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
            if let Some(delay) = self.delay {
                sleep(delay).await;
            }
            Ok(self.matches.clone())
        }

        async fn invoke(
            &self,
            _a: &quantum_domain::Action,
        ) -> std::result::Result<quantum_domain::ActionOutcome, DomainError> {
            Ok(quantum_domain::ActionOutcome { message: None })
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

        fn with_provider(mut self, id: ProviderId, matches: Vec<Match>) -> Self {
            let provider = FakeProvider {
                id: id.clone(),
                matches,
                delay: None,
            };
            self.providers.insert(id, Arc::new(provider));
            self
        }

        fn with_slow_provider(mut self, id: ProviderId, delay: StdDuration) -> Self {
            let provider = FakeProvider {
                id: id.clone(),
                matches: vec![],
                delay: Some(delay),
            };
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

    fn test_match(id: &str, title: &str, score: f32) -> Match {
        Match {
            id: id.to_string(),
            provider: "test".into(),
            title: title.to_string(),
            subtitle: None,
            icon: None,
            score: MatchScore::new(score),
            action: Action::Launch {
                desktop_id: "test".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn aggregates_from_multiple_providers() {
        let reg = FakeRegistry::new()
            .with_provider("apps".into(), vec![test_match("a", "Firefox", 0.9)])
            .with_provider("hypr".into(), vec![test_match("b", "Editor", 0.7)]);

        let uc = SearchUseCase::new(Arc::new(reg));
        let res = uc.execute(Query::new("x")).await.unwrap();

        assert_eq!(res.matches.len(), 2);
        assert_eq!(res.matches[0].id, "a"); // higher score first
        assert_eq!(res.matches[1].id, "b");
        assert!(res.warnings.is_empty());
    }

    #[tokio::test]
    async fn timeouts_become_warnings() {
        let reg = FakeRegistry::new().with_slow_provider("apps".into(), StdDuration::from_secs(3));

        let uc = SearchUseCase::new(Arc::new(reg));
        let res = uc.execute(Query::new("x")).await.unwrap();

        assert!(res.matches.is_empty());
        assert_eq!(res.warnings.len(), 1);
        assert!(res.warnings[0].contains("apps"));
        assert!(res.warnings[0].contains("timed out"));
    }

    #[tokio::test]
    async fn missing_provider_produces_warning() {
        let query = Query {
            text: "x".to_string(),
            providers: vec!["nonexistent".into()],
            limit: None,
        };

        let reg =
            FakeRegistry::new().with_provider("apps".into(), vec![test_match("a", "Firefox", 0.9)]);

        let uc = SearchUseCase::new(Arc::new(reg));
        let res = uc.execute(query).await.unwrap();

        assert!(res.matches.is_empty());
        assert_eq!(res.warnings.len(), 1);
        assert!(res.warnings[0].contains("not found"));
    }

    #[tokio::test]
    async fn results_sorted_by_score_descending() {
        let reg = FakeRegistry::new().with_provider(
            "app".into(),
            vec![
                test_match("a", "Low", 0.3),
                test_match("b", "High", 0.9),
                test_match("c", "Mid", 0.5),
            ],
        );

        let uc = SearchUseCase::new(Arc::new(reg));
        let res = uc.execute(Query::new("x")).await.unwrap();

        assert_eq!(res.matches.len(), 3);
        assert_eq!(res.matches[0].id, "b"); // 0.9
        assert_eq!(res.matches[1].id, "c"); // 0.5
        assert_eq!(res.matches[2].id, "a"); // 0.3
    }

    #[tokio::test]
    async fn limit_honoured() {
        let reg = FakeRegistry::new().with_provider(
            "app".into(),
            vec![
                test_match("a", "Match 1", 0.9),
                test_match("b", "Match 2", 0.8),
                test_match("c", "Match 3", 0.7),
            ],
        );

        let query = Query {
            text: "x".to_string(),
            providers: vec!["app".into()],
            limit: Some(2),
        };

        let uc = SearchUseCase::new(Arc::new(reg));
        let res = uc.execute(query).await.unwrap();

        assert_eq!(res.matches.len(), 2);
    }

    #[tokio::test]
    async fn empty_providers_list_searches_all() {
        let reg = FakeRegistry::new()
            .with_provider("a".into(), vec![test_match("a1", "From A", 0.9)])
            .with_provider("b".into(), vec![test_match("b1", "From B", 0.8)]);

        let uc = SearchUseCase::new(Arc::new(reg));
        let res = uc.execute(Query::new("x")).await.unwrap();

        assert_eq!(res.matches.len(), 2);
    }
}
