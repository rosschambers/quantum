use async_trait::async_trait;
use std::sync::Arc;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, MatchScore, ProviderId, ProviderSource, Query,
    ShellExecutor,
};

/// Provider for shell commands (queries starting with ">").
pub struct ShellCommandProvider {
    id: ProviderId,
    executor: Arc<dyn ShellExecutor>,
}

impl ShellCommandProvider {
    /// Create a new ShellCommandProvider.
    pub fn new(executor: Arc<dyn ShellExecutor>) -> Self {
        Self {
            id: ProviderId::from("shell"),
            executor,
        }
    }
}

#[async_trait]
impl ProviderSource for ShellCommandProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError> {
        // Only match if query starts with ">"
        if !q.text.starts_with('>') {
            return Ok(vec![]);
        }

        let command = q.text[1..].trim();

        if command.is_empty() {
            return Ok(vec![]);
        }

        Ok(vec![Match {
            id: "shell_command".to_string(),
            provider: self.id.clone(),
            title: format!("Execute: {}", command),
            subtitle: Some("Shell command".to_string()),
            icon: None,
            score: MatchScore::new(1.0),
            action: Action::Shell {
                command: vec!["sh".to_string(), "-c".to_string(), command.to_string()],
                terminal: false,
            },
        }])
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Shell {
                command,
                terminal: _,
            } => {
                self.executor.spawn_detached(command).await?;
                Ok(ActionOutcome {
                    message: Some("Command executed".to_string()),
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
        spawned: Arc<tokio::sync::RwLock<Vec<Vec<String>>>>,
    }

    impl FakeExecutor {
        fn new() -> Self {
            Self {
                spawned: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl ShellExecutor for FakeExecutor {
        async fn run_with_timeout(
            &self,
            _command: &[String],
            _timeout_ms: u64,
        ) -> Result<quantum_domain::ShellOutput, DomainError> {
            Ok(quantum_domain::ShellOutput {
                stdout: String::new(),
                stderr: String::new(),
                status: 0,
            })
        }

        async fn spawn_detached(&self, command: &[String]) -> Result<(), DomainError> {
            self.spawned.write().await.push(command.to_vec());
            Ok(())
        }
    }

    #[tokio::test]
    async fn search_query_starting_with_gt_returns_match() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = ShellCommandProvider::new(executor);

        let query = Query::new(">echo hello");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(matches.len(), 1);
        assert!(matches[0].title.contains("echo hello"));
    }

    #[tokio::test]
    async fn search_query_not_starting_with_gt_returns_empty() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = ShellCommandProvider::new(executor);

        let query = Query::new("echo hello");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(matches.len(), 0);
    }

    #[tokio::test]
    async fn search_empty_command_returns_empty() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = ShellCommandProvider::new(executor);

        let query = Query::new(">   ");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(matches.len(), 0);
    }

    #[tokio::test]
    async fn invoke_shell_action() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = ShellCommandProvider::new(executor.clone());

        let action = Action::Shell {
            command: vec!["echo".to_string(), "test".to_string()],
            terminal: false,
        };
        let result = provider.invoke(&action).await.unwrap();

        assert!(result.message.is_some());

        let spawned = executor.spawned.read().await;
        assert!(!spawned.is_empty());
        assert_eq!(spawned[0], vec!["echo".to_string(), "test".to_string()]);
    }

    #[tokio::test]
    async fn invoke_non_shell_action_fails() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = ShellCommandProvider::new(executor);

        let action = Action::Launch {
            desktop_id: "test".to_string(),
        };
        let result = provider.invoke(&action).await;

        assert!(result.is_err());
    }
}
