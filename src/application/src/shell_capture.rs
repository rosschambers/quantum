use quantum_domain::{DomainError, NotificationEmitter, ShellCaptureResult, ShellExecutor};
use std::sync::Arc;

/// Maximum number of characters shown in a notification summary (the command).
const SUMMARY_MAX_CHARS: usize = 60;

/// Maximum number of characters shown in a notification body (stdout/stderr).
const BODY_MAX_CHARS: usize = 500;

/// Where a captured command's output should be delivered. `Inline` means the
/// result is handed straight back to the caller (the launcher renders it in a
/// panel); `Notification` means it is also surfaced as a user-facing
/// notification through the [`NotificationEmitter`] port.
#[derive(Debug, PartialEq, Eq)]
pub enum CaptureSink {
    Inline,
    Notification,
}

/// Runs a launcher command through the domain [`ShellExecutor`] port, captures
/// its output into a [`ShellCaptureResult`], and routes that result through the
/// single output-routing policy in [`ShellCaptureUseCase::capture_sinks`].
pub struct ShellCaptureUseCase {
    executor: Arc<dyn ShellExecutor>,
    emitter: Arc<dyn NotificationEmitter>,
    timeout_ms: u64,
}

impl ShellCaptureUseCase {
    pub fn new(
        executor: Arc<dyn ShellExecutor>,
        emitter: Arc<dyn NotificationEmitter>,
        timeout_ms: u64,
    ) -> Self {
        Self {
            executor,
            emitter,
            timeout_ms,
        }
    }

    /// Run `sh -c "<command>"` with the configured timeout, build a
    /// [`ShellCaptureResult`], apply the routing policy, and return the result.
    ///
    /// Never returns `Err` and never panics: the caller (the UI) always gets a
    /// result. A failure to run the command is folded into the result itself
    /// (a timeout sets `timed_out`; any other failure carries its text in
    /// `stderr` with `exit_code` `-1`).
    pub async fn run(&self, command: &str) -> ShellCaptureResult {
        let argv = ["sh".to_string(), "-c".to_string(), command.to_string()];

        let result = match self.executor.run_with_timeout(&argv, self.timeout_ms).await {
            Ok(output) => ShellCaptureResult {
                command: command.to_string(),
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.status,
                timed_out: false,
            },
            Err(error) if is_timeout(&error) => ShellCaptureResult {
                command: command.to_string(),
                stdout: String::new(),
                stderr: String::new(),
                exit_code: -1,
                timed_out: true,
            },
            Err(error) => ShellCaptureResult {
                command: command.to_string(),
                stdout: String::new(),
                stderr: error.to_string(),
                exit_code: -1,
                timed_out: false,
            },
        };

        self.route_capture_output(&result).await;
        result
    }

    /// Deliver a built result to each configured sink. `Inline` is a no-op
    /// because the result is already returned to the caller; `Notification`
    /// emits a summary and body through the [`NotificationEmitter`] port.
    async fn route_capture_output(&self, result: &ShellCaptureResult) {
        for sink in Self::capture_sinks(result) {
            match sink {
                CaptureSink::Inline => {}
                CaptureSink::Notification => {
                    self.emitter
                        .emit(&notification_summary(result), &notification_body(result))
                        .await;
                }
            }
        }
    }

    /// THE single output-routing policy. Every captured result currently goes
    /// to both the inline panel and a notification.
    ///
    /// This is the ONE place to change to make routing selectable later (per
    /// the design's switchable-policy constraint): return a different set of
    /// sinks here — for example keyed off the result, a configuration flag, or
    /// a per-command directive — and the rest of the use case follows without
    /// modification. Keep the switch here; do not scatter routing decisions
    /// across `run`/`route_capture_output`.
    pub fn capture_sinks(_result: &ShellCaptureResult) -> Vec<CaptureSink> {
        vec![CaptureSink::Inline, CaptureSink::Notification]
    }
}

/// Whether a `run_with_timeout` error represents an exceeded deadline.
///
/// The domain error type has no dedicated timeout variant, so the
/// infrastructure `TokioShellExecutor::run_with_timeout` reports a timeout as
/// the sentinel `DomainError::Unsupported("command timeout")`
/// (`src/infrastructure/providers/src/shell.rs`), distinct from a spawn failure
/// (`Unsupported("spawn: ...")`). This matches that exact sentinel text so a
/// timeout is told apart from other failures.
fn is_timeout(error: &DomainError) -> bool {
    matches!(error, DomainError::Unsupported(message) if message == "command timeout")
}

/// The notification summary: the command, truncated for a compact heading.
fn notification_summary(result: &ShellCaptureResult) -> String {
    truncate(&result.command, SUMMARY_MAX_CHARS)
}

/// The notification body. On success (`exit_code == 0` and not timed out) it is
/// the standard output, truncated, or `"(no output)"` when that output is empty
/// after trimming. On failure it leads with `"timed out"` or `"exit <code>"`
/// and appends the (truncated) standard error when present.
fn notification_body(result: &ShellCaptureResult) -> String {
    if result.exit_code == 0 && !result.timed_out {
        if result.stdout.trim().is_empty() {
            return "(no output)".to_string();
        }
        return truncate(&result.stdout, BODY_MAX_CHARS);
    }

    let mut body = if result.timed_out {
        "timed out".to_string()
    } else {
        format!("exit {}", result.exit_code)
    };

    if !result.stderr.trim().is_empty() {
        body.push('\n');
        body.push_str(&truncate(&result.stderr, BODY_MAX_CHARS));
    }

    body
}

/// Truncate `text` to at most `max` characters, appending a single ellipsis
/// character when it was shortened. Counts and slices by characters, never
/// bytes, so it never splits a multi-byte UTF-8 sequence.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(max).collect();
    truncated.push('\u{2026}');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::{DomainError, ShellOutput};
    use std::sync::Mutex;

    /// A [`ShellExecutor`] that returns a preconfigured outcome for
    /// `run_with_timeout`, so a use-case test never touches a real process.
    struct FakeShellExecutor {
        outcome: Result<ShellOutput, DomainError>,
    }

    impl FakeShellExecutor {
        fn ok(stdout: &str, stderr: &str, status: i32) -> Self {
            Self {
                outcome: Ok(ShellOutput {
                    stdout: stdout.to_string(),
                    stderr: stderr.to_string(),
                    status,
                }),
            }
        }

        fn err(error: DomainError) -> Self {
            Self {
                outcome: Err(error),
            }
        }
    }

    #[async_trait]
    impl ShellExecutor for FakeShellExecutor {
        async fn run_with_timeout(
            &self,
            _command: &[String],
            _timeout_ms: u64,
        ) -> Result<ShellOutput, DomainError> {
            self.outcome.clone()
        }

        async fn spawn_detached(&self, _command: &[String]) -> Result<(), DomainError> {
            Ok(())
        }
    }

    /// A [`NotificationEmitter`] that records every emitted `(summary, body)`
    /// pair so a test can assert what was surfaced.
    #[derive(Default)]
    struct SpyNotificationEmitter {
        emitted: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl SpyNotificationEmitter {
        fn new() -> Self {
            Self::default()
        }

        fn emitted(&self) -> Arc<Mutex<Vec<(String, String)>>> {
            Arc::clone(&self.emitted)
        }
    }

    #[async_trait]
    impl NotificationEmitter for SpyNotificationEmitter {
        async fn emit(&self, summary: &str, body: &str) {
            self.emitted
                .lock()
                .expect("emitted lock")
                .push((summary.to_string(), body.to_string()));
        }
    }

    #[tokio::test]
    async fn success_returns_result_and_emits_notification() {
        let executor = Arc::new(FakeShellExecutor::ok("hi\n", "", 0));
        let emitter = Arc::new(SpyNotificationEmitter::new());
        let recorded = emitter.emitted();
        let use_case = ShellCaptureUseCase::new(executor, emitter, 10_000);

        let result = use_case.run("echo hi").await;

        assert_eq!(result.command, "echo hi");
        assert_eq!(result.stdout, "hi\n");
        assert_eq!(result.exit_code, 0);
        assert!(!result.timed_out);

        let emitted = recorded.lock().expect("emitted lock");
        assert_eq!(emitted.len(), 1);
        assert!(
            emitted[0].1.contains("hi"),
            "body should contain stdout, got {:?}",
            emitted[0].1
        );
    }

    #[tokio::test]
    async fn non_zero_exit_emits_failure_body() {
        let executor = Arc::new(FakeShellExecutor::ok("", "boom", 2));
        let emitter = Arc::new(SpyNotificationEmitter::new());
        let recorded = emitter.emitted();
        let use_case = ShellCaptureUseCase::new(executor, emitter, 10_000);

        let result = use_case.run("failing-command").await;

        assert_eq!(result.exit_code, 2);
        assert!(!result.timed_out);

        let emitted = recorded.lock().expect("emitted lock");
        assert_eq!(emitted.len(), 1);
        let body = &emitted[0].1;
        assert!(body.contains("exit 2"), "body should note exit 2: {body:?}");
        assert!(body.contains("boom"), "body should carry stderr: {body:?}");
    }

    #[tokio::test]
    async fn timeout_sets_timed_out_and_emits() {
        let executor = Arc::new(FakeShellExecutor::err(DomainError::Unsupported(
            "command timeout".to_string(),
        )));
        let emitter = Arc::new(SpyNotificationEmitter::new());
        let recorded = emitter.emitted();
        let use_case = ShellCaptureUseCase::new(executor, emitter, 10_000);

        let result = use_case.run("sleep 30").await;

        assert!(result.timed_out);
        assert_eq!(result.exit_code, -1);
        assert_eq!(result.command, "sleep 30");

        let emitted = recorded.lock().expect("emitted lock");
        assert_eq!(emitted.len(), 1);
        assert!(
            emitted[0].1.contains("timed out"),
            "body should note the timeout: {:?}",
            emitted[0].1
        );
    }

    #[tokio::test]
    async fn other_error_carries_error_text_without_timeout() {
        let executor = Arc::new(FakeShellExecutor::err(DomainError::Unsupported(
            "spawn: No such file or directory".to_string(),
        )));
        let emitter = Arc::new(SpyNotificationEmitter::new());
        let use_case = ShellCaptureUseCase::new(executor, emitter, 10_000);

        let result = use_case.run("nope").await;

        assert!(!result.timed_out);
        assert_eq!(result.exit_code, -1);
        assert!(
            result.stderr.contains("spawn: No such file or directory"),
            "stderr should carry the error text: {:?}",
            result.stderr
        );
    }

    #[test]
    fn capture_sinks_returns_inline_then_notification() {
        let result = ShellCaptureResult {
            command: "echo hi".to_string(),
            stdout: "hi\n".to_string(),
            stderr: String::new(),
            exit_code: 0,
            timed_out: false,
        };
        let sinks = ShellCaptureUseCase::capture_sinks(&result);
        assert_eq!(sinks, vec![CaptureSink::Inline, CaptureSink::Notification]);
    }

    #[test]
    fn notification_summary_truncates_long_command() {
        let command = "a".repeat(200);
        let result = ShellCaptureResult {
            command: command.clone(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            timed_out: false,
        };
        let summary = notification_summary(&result);
        assert!(
            summary.chars().count() <= 61,
            "summary should be truncated near 60 chars, got {}",
            summary.chars().count()
        );
        assert!(
            summary.ends_with('…'),
            "truncated summary ends with ellipsis"
        );
    }

    #[test]
    fn notification_body_returns_no_output_for_empty_success() {
        let result = ShellCaptureResult {
            command: "true".to_string(),
            stdout: "   \n".to_string(),
            stderr: String::new(),
            exit_code: 0,
            timed_out: false,
        };
        assert_eq!(notification_body(&result), "(no output)");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        // Five three-byte characters; truncating to 3 must not split a byte
        // sequence and must append the single-character ellipsis.
        let text = "\u{03b1}\u{03b1}\u{03b1}\u{03b1}\u{03b1}";
        let truncated = truncate(text, 3);
        assert_eq!(truncated, "\u{03b1}\u{03b1}\u{03b1}\u{2026}");
        assert_eq!(truncated.chars().count(), 4);
    }

    #[test]
    fn truncate_leaves_short_text_untouched() {
        assert_eq!(truncate("short", 60), "short");
    }
}
