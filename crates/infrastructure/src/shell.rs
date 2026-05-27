use async_trait::async_trait;
use std::time::Duration;
use tokio::process::Command;

use quantum_domain::{DomainError, ShellExecutor, ShellOutput};

use crate::InfrastructureError;

/// Tokio-based shell executor for running commands with timeouts.
pub struct TokioShellExecutor;

impl TokioShellExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TokioShellExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ShellExecutor for TokioShellExecutor {
    async fn execute(&self, _command: &[String]) -> Result<String, DomainError> {
        Err(DomainError::Unsupported("use run_with_timeout instead".to_string()))
    }

    async fn run_with_timeout(
        &self,
        command: &[String],
        timeout_ms: u64,
    ) -> Result<ShellOutput, DomainError> {
        if command.is_empty() {
            return Err(DomainError::InvalidQuery("empty command".to_string()));
        }

        let timeout_duration = Duration::from_millis(timeout_ms);
        let mut cmd = Command::new(&command[0]);

        for arg in &command[1..] {
            cmd.arg(arg);
        }

        let future = async {
            cmd.output()
                .await
                .map_err(|e| InfrastructureError::Spawn(e.to_string()))
        };

        match tokio::time::timeout(timeout_duration, future).await {
            Ok(Ok(output)) => Ok(ShellOutput {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                status: output.status.code().unwrap_or(-1),
            }),
            Ok(Err(e)) => Err(e.to_domain()),
            Err(_) => Err(DomainError::Unsupported("command timeout".to_string())),
        }
    }

    async fn spawn_detached(&self, command: &[String]) -> Result<(), DomainError> {
        if command.is_empty() {
            return Err(DomainError::InvalidQuery("empty command".to_string()));
        }

        let mut cmd = Command::new(&command[0]);

        for arg in &command[1..] {
            cmd.arg(arg);
        }

        cmd.spawn()
            .map_err(|e| DomainError::Unsupported(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_true_returns_zero_status() {
        let executor = TokioShellExecutor::new();
        let output = executor
            .run_with_timeout(&["true".to_string()], 5000)
            .await
            .unwrap();
        assert_eq!(output.status, 0);
    }

    #[tokio::test]
    async fn run_false_returns_nonzero_status() {
        let executor = TokioShellExecutor::new();
        let output = executor
            .run_with_timeout(&["false".to_string()], 5000)
            .await
            .unwrap();
        assert_ne!(output.status, 0);
    }

    #[tokio::test]
    async fn run_echo_returns_stdout() {
        let executor = TokioShellExecutor::new();
        let output = executor
            .run_with_timeout(
                &["sh".to_string(), "-c".to_string(), "echo hello".to_string()],
                5000,
            )
            .await
            .unwrap();
        assert_eq!(output.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn timeout_returns_error() {
        let executor = TokioShellExecutor::new();
        let result = executor
            .run_with_timeout(
                &["sleep".to_string(), "5".to_string()],
                100, // 100ms timeout for 5s sleep
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn spawn_detached_succeeds() {
        let executor = TokioShellExecutor::new();
        let result = executor
            .spawn_detached(&["true".to_string()])
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn empty_command_returns_error() {
        let executor = TokioShellExecutor::new();
        let result = executor.run_with_timeout(&[], 5000).await;
        assert!(result.is_err());
    }
}
