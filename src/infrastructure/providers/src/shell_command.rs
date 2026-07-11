use async_trait::async_trait;
use std::sync::Arc;

use quantum_domain::{
    Action, ActionOutcome, DomainError, Match, MatchScore, ProviderId, ProviderSource, Query,
    ShellExecutor,
};

/// Provider for shell commands. A query starting with `>` runs the command
/// detached in the background; a query starting with `!` runs it inside a
/// terminal window.
pub struct ShellCommandProvider {
    id: ProviderId,
    executor: Arc<dyn ShellExecutor>,
}

/// Resolve the argv to run `shell_command` inside a terminal emulator. The
/// terminal program comes from `terminal_program` (read from the `TERMINAL`
/// environment variable by the caller): when it is `Some` and non-empty that
/// program is used, otherwise it defaults to `xterm`. The result is
/// `[program, "-e", <shell_command...>]`, the `-e` convention honored by
/// alacritty, foot, kitty, xterm, and gnome-terminal.
fn terminal_command(terminal_program: Option<&str>, shell_command: &[String]) -> Vec<String> {
    let program = match terminal_program {
        Some(program) if !program.is_empty() => program,
        _ => "xterm",
    };
    let mut argv = vec![program.to_string(), "-e".to_string()];
    argv.extend(shell_command.iter().cloned());
    argv
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
        // A `>` prefix runs the command detached; a `!` prefix runs it in a
        // terminal window. Anything else is not a shell command.
        let (rest, terminal) = if let Some(rest) = q.text.strip_prefix('!') {
            (rest, true)
        } else if let Some(rest) = q.text.strip_prefix('>') {
            (rest, false)
        } else {
            return Ok(vec![]);
        };

        let command = rest.trim();

        if command.is_empty() {
            return Ok(vec![]);
        }

        let subtitle = if terminal {
            "Run in terminal"
        } else {
            "Shell command"
        };

        Ok(vec![Match {
            id: "shell_command".to_string(),
            provider: self.id.clone(),
            title: format!("Run: {}", command),
            subtitle: Some(subtitle.to_string()),
            icon: None,
            score: MatchScore::new(1.0),
            action: Action::Shell {
                command: vec!["sh".to_string(), "-c".to_string(), command.to_string()],
                terminal,
            },
        }])
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Shell { command, terminal } => {
                if *terminal {
                    // Wrap the command in the user's terminal (from `TERMINAL`,
                    // falling back to xterm) and launch that detached.
                    let terminal_program = std::env::var("TERMINAL").ok();
                    let argv = terminal_command(terminal_program.as_deref(), command);
                    self.executor.spawn_detached(&argv).await?;
                } else {
                    self.executor.spawn_detached(command).await?;
                }
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
    async fn search_query_starting_with_gt_runs_detached() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = ShellCommandProvider::new(executor);

        let query = Query::new(">echo hello");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(matches.len(), 1);
        assert!(matches[0].title.starts_with("Run:"));
        assert!(matches[0].title.contains("echo hello"));
        match &matches[0].action {
            Action::Shell { command, terminal } => {
                assert!(!terminal);
                assert_eq!(
                    command,
                    &vec!["sh".to_string(), "-c".to_string(), "echo hello".to_string()]
                );
            }
            _ => panic!("expected Shell action"),
        }
    }

    #[tokio::test]
    async fn search_query_starting_with_bang_runs_in_terminal() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = ShellCommandProvider::new(executor);

        let query = Query::new("!htop");
        let matches = provider.search(&query).await.unwrap();

        assert_eq!(matches.len(), 1);
        assert!(matches[0].title.starts_with("Run:"));
        match &matches[0].action {
            Action::Shell { command, terminal } => {
                assert!(terminal);
                assert_eq!(
                    command,
                    &vec!["sh".to_string(), "-c".to_string(), "htop".to_string()]
                );
            }
            _ => panic!("expected Shell action"),
        }
    }

    #[tokio::test]
    async fn search_query_not_starting_with_prefix_returns_empty() {
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

        assert_eq!(provider.search(&Query::new(">   ")).await.unwrap().len(), 0);
        assert_eq!(provider.search(&Query::new("!   ")).await.unwrap().len(), 0);
    }

    #[test]
    fn terminal_command_uses_provided_program() {
        let shell_command = vec!["sh".to_string(), "-c".to_string(), "kill x".to_string()];
        assert_eq!(
            terminal_command(Some("alacritty"), &shell_command),
            vec![
                "alacritty".to_string(),
                "-e".to_string(),
                "sh".to_string(),
                "-c".to_string(),
                "kill x".to_string(),
            ]
        );
    }

    #[test]
    fn terminal_command_defaults_to_xterm_when_none() {
        let shell_command = vec!["sh".to_string(), "-c".to_string(), "kill x".to_string()];
        assert_eq!(
            terminal_command(None, &shell_command),
            vec![
                "xterm".to_string(),
                "-e".to_string(),
                "sh".to_string(),
                "-c".to_string(),
                "kill x".to_string(),
            ]
        );
    }

    #[test]
    fn terminal_command_falls_back_to_xterm_when_empty() {
        let shell_command = vec!["sh".to_string(), "-c".to_string(), "kill x".to_string()];
        assert_eq!(
            terminal_command(Some(""), &shell_command),
            vec![
                "xterm".to_string(),
                "-e".to_string(),
                "sh".to_string(),
                "-c".to_string(),
                "kill x".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn invoke_terminal_action_spawns_terminal_wrapped_argv() {
        let executor = Arc::new(FakeExecutor::new());
        let provider = ShellCommandProvider::new(executor.clone());

        std::env::set_var("TERMINAL", "alacritty");
        let action = Action::Shell {
            command: vec![
                "sh".to_string(),
                "-c".to_string(),
                "kill quantumd".to_string(),
            ],
            terminal: true,
        };
        let result = provider.invoke(&action).await.unwrap();
        std::env::remove_var("TERMINAL");

        assert!(result.message.is_some());
        let spawned = executor.spawned.read().await;
        assert_eq!(
            spawned[0],
            vec![
                "alacritty".to_string(),
                "-e".to_string(),
                "sh".to_string(),
                "-c".to_string(),
                "kill quantumd".to_string(),
            ]
        );
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
