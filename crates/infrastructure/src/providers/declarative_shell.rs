use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, MatchScore, ProviderCapabilities, ProviderId,
    ProviderSource, Query, ShellExecutor,
};

use crate::config::ProviderConfig;

/// Provider that runs commands and parses output declaratively.
pub struct DeclarativeShellProvider {
    id: ProviderId,
    config: ProviderConfig,
    executor: Arc<dyn ShellExecutor>,
    cached_lines: RwLock<Vec<String>>,
}

impl DeclarativeShellProvider {
    /// Create a new DeclarativeShellProvider from a config.
    pub fn new(
        config: ProviderConfig,
        executor: Arc<dyn ShellExecutor>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id: ProviderId::from(config.id.clone()),
            config,
            executor,
            cached_lines: RwLock::new(Vec::new()),
        })
    }

    /// Execute the configured command and cache results.
    async fn refresh_cache(&self) -> Result<(), DomainError> {
        let output = self
            .executor
            .run_with_timeout(&self.config.command, 5000)
            .await?;

        let lines: Vec<String> = output
            .stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();

        *self.cached_lines.write().await = lines;
        Ok(())
    }
}

#[async_trait]
impl ProviderSource for DeclarativeShellProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            searchable: true,
            streamable: false,
        }
    }

    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError> {
        self.refresh_cache().await?;

        let lines = self.cached_lines.read().await;
        let query_lower = q.text.to_lowercase();
        let mut matches = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            let line_lower = line.to_lowercase();

            // Substring match
            if line_lower.contains(&query_lower) {
                let title = if let Some(template) = &self.config.match_template {
                    template
                        .title
                        .as_ref()
                        .map(|t| t.replace("{0}", line))
                        .unwrap_or_else(|| line.clone())
                } else {
                    line.clone()
                };

                let subtitle = self
                    .config
                    .match_template
                    .as_ref()
                    .and_then(|t| t.subtitle.clone());

                let score = if line_lower.starts_with(&query_lower) {
                    1.0
                } else {
                    0.7
                };

                matches.push(Match {
                    id: format!("{}_{}", self.id, idx),
                    provider: self.id.clone(),
                    title,
                    subtitle,
                    icon: None,
                    score: MatchScore::new(score),
                    action: self.config.action.clone().unwrap_or(Action::Shell {
                        command: vec!["true".to_string()],
                        terminal: false,
                    }),
                });
            }
        }

        // Limit results
        if let Some(limit) = q.limit {
            matches.truncate(limit as usize);
        }

        Ok(matches)
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Shell {
                command,
                terminal: _,
            } => {
                self.executor.spawn_detached(command).await?;
                Ok(ActionOutcome {
                    message: Some("Executed".to_string()),
                })
            }
            _ => Err(DomainError::Unsupported(
                "only Shell action supported".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeExecutor {
        output: String,
    }

    impl FakeExecutor {
        fn new(output: String) -> Self {
            Self { output }
        }
    }

    #[async_trait]
    impl ShellExecutor for FakeExecutor {
        async fn execute(&self, _command: &[String]) -> Result<String, DomainError> {
            Ok(self.output.clone())
        }

        async fn run_with_timeout(
            &self,
            _command: &[String],
            _timeout_ms: u64,
        ) -> Result<quantum_domain::ShellOutput, DomainError> {
            Ok(quantum_domain::ShellOutput {
                stdout: self.output.clone(),
                stderr: String::new(),
                status: 0,
            })
        }

        async fn spawn_detached(&self, _command: &[String]) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn search_filters_by_substring() {
        let config = ProviderConfig {
            id: "test".to_string(),
            command: vec!["echo".to_string(), "test".to_string()],
            refresh: None,
            parse: Some("lines".to_string()),
            match_template: None,
            action: None,
        };

        let executor = Arc::new(FakeExecutor::new("foo\nbar\nbaz\nfoo-extra".to_string()));
        let provider = DeclarativeShellProvider::new(config, executor).unwrap();

        let query = Query::new("foo");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(matches.len(), 2); // "foo" and "foo-extra"
        assert_eq!(matches[0].title, "foo");
    }

    #[tokio::test]
    async fn invoke_shell_action() {
        let config = ProviderConfig {
            id: "test".to_string(),
            command: vec!["echo".to_string()],
            refresh: None,
            parse: None,
            match_template: None,
            action: Some(Action::Shell {
                command: vec!["true".to_string()],
                terminal: false,
            }),
        };

        let executor = Arc::new(FakeExecutor::new(String::new()));
        let provider = DeclarativeShellProvider::new(config, executor).unwrap();

        let action = Action::Shell {
            command: vec!["echo".to_string(), "test".to_string()],
            terminal: false,
        };
        let result = provider.invoke(&action).await.unwrap();

        assert!(result.message.is_some());
    }

    #[tokio::test]
    async fn search_with_template() {
        let config = ProviderConfig {
            id: "test".to_string(),
            command: vec!["echo".to_string()],
            refresh: None,
            parse: Some("lines".to_string()),
            match_template: Some(crate::config::loader::MatchTemplate {
                title: Some("Item: {0}".to_string()),
                subtitle: Some("description".to_string()),
            }),
            action: None,
        };

        let executor = Arc::new(FakeExecutor::new("test1\ntest2".to_string()));
        let provider = DeclarativeShellProvider::new(config, executor).unwrap();

        let query = Query::new("test");
        let matches = provider.search(&query).await.unwrap();

        assert!(!matches.is_empty());
        assert!(matches[0].title.contains("Item: test"));
    }
}
