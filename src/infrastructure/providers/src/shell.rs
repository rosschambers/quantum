use async_trait::async_trait;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use quantum_domain::{DomainError, ShellExecutor, ShellOutput};

use crate::error::ProvidersError;

/// Wrap a launch argv in a transient systemd user scope so the spawned
/// application lands in its own cgroup, independent of `quantum.service`.
/// When `use_scope` is false (systemd-run unavailable), the argv is returned
/// unchanged for a direct spawn.
fn scope_wrapped_argv(command: &[String], use_scope: bool) -> Vec<String> {
    if !use_scope {
        return command.to_vec();
    }
    let mut argv = vec![
        "systemd-run".to_string(),
        "--user".to_string(),
        "--scope".to_string(),
        "--quiet".to_string(),
        "--collect".to_string(),
        "--".to_string(),
    ];
    argv.extend_from_slice(command);
    argv
}

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
                .map_err(|e| ProvidersError::Spawn(e.to_string()))
        };

        match tokio::time::timeout(timeout_duration, future).await {
            Ok(Ok(output)) => Ok(ShellOutput {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                status: output.status.code().unwrap_or(-1),
            }),
            Ok(Err(err)) => {
                let ProvidersError::Spawn(msg) = err else {
                    unreachable!("inner future only yields ProvidersError::Spawn")
                };
                Err(DomainError::Unsupported(format!("spawn: {}", msg)))
            }
            Err(_) => Err(DomainError::Unsupported("command timeout".to_string())),
        }
    }

    async fn spawn_detached(&self, command: &[String]) -> Result<(), DomainError> {
        if command.is_empty() {
            return Err(DomainError::InvalidQuery("empty command".to_string()));
        }

        // If QUANTUM_SHELL_LOG is set, log the command to that file instead of executing
        if let Ok(log_path) = std::env::var("QUANTUM_SHELL_LOG") {
            let command_str = serde_json::json!({
                "command": command,
                "timestamp": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0)
            });
            let line = serde_json::to_string(&command_str)
                .map_err(|e| DomainError::Unsupported(format!("json serialization failed: {e}")))?;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                // Opt-in debug log that records every launched command; keep it
                // readable and writable only by the owner (0600) rather than
                // whatever the process umask would grant.
                .mode(0o600)
                .open(&log_path)
                .and_then(|mut f| {
                    use std::io::Write;
                    writeln!(f, "{}", line)
                })
                .map_err(|e| DomainError::Unsupported(format!("failed to write shell log: {e}")))?;
            return Ok(());
        }

        let mut cmd = Command::new(&command[0]);

        for arg in &command[1..] {
            cmd.arg(arg);
        }

        // Detach the child from the daemon: its own process group so signals
        // sent to the daemon's group never reach it, and null stdio so it
        // neither inherits the daemon's descriptors nor holds them open. The
        // session environment (WAYLAND_DISPLAY, DBUS_SESSION_BUS_ADDRESS, PATH,
        // and so on) is left intact because launched GUI applications need it.
        cmd.process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| DomainError::Unsupported(e.to_string()))?;
        tokio::spawn(async move {
            let _ = child.wait().await;
        });

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
        let result = executor.spawn_detached(&["true".to_string()]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn empty_command_returns_error() {
        let executor = TokioShellExecutor::new();
        let result = executor.run_with_timeout(&[], 5000).await;
        assert!(result.is_err());
    }

    #[test]
    fn scope_wrapped_argv_prefixes_systemd_run_when_enabled() {
        let cmd = vec!["firefox".to_string(), "--new-window".to_string()];
        assert_eq!(
            super::scope_wrapped_argv(&cmd, true),
            vec![
                "systemd-run",
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--",
                "firefox",
                "--new-window",
            ]
        );
    }

    #[test]
    fn scope_wrapped_argv_passes_through_when_disabled() {
        let cmd = vec!["firefox".to_string()];
        assert_eq!(super::scope_wrapped_argv(&cmd, false), cmd);
    }
}
