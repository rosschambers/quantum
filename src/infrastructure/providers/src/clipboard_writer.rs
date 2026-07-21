use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use quantum_domain::{ClipboardWriter, DomainError};

/// Writes to the system clipboard by shelling out to `wl-copy` (Wayland).
///
/// Holds a resolved copy argv as the program plus any fixed leading arguments;
/// `write_text` appends the text as the final argument, while `write_bytes`
/// pipes the payload to the child's standard input under an explicit MIME type.
pub struct WlClipboardWriter {
    program: Vec<String>,
}

impl WlClipboardWriter {
    /// Construct a writer around a copy program argv. `None` or an empty vec
    /// defaults to `["wl-copy"]`.
    pub fn new(copy_program: Option<Vec<String>>) -> Self {
        let program = match copy_program {
            Some(program) if !program.is_empty() => program,
            _ => vec!["wl-copy".to_string()],
        };
        Self { program }
    }

    /// The resolved copy program argv (program plus any fixed leading args).
    pub fn program(&self) -> &[String] {
        &self.program
    }
}

/// Build the argv for a plain-text copy: the program argv followed by `text`
/// as the final argument. Pure; spawns nothing.
fn copy_text_argv(program: &[String], text: &str) -> Vec<String> {
    let mut argv = program.to_vec();
    argv.push(text.to_string());
    argv
}

#[async_trait]
impl ClipboardWriter for WlClipboardWriter {
    async fn write_text(&self, text: &str) -> Result<(), DomainError> {
        let argv = copy_text_argv(&self.program, text);
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);

        let mut child = command.spawn().map_err(|error| {
            tracing::warn!(program = %argv[0], %error, "failed to spawn wl-copy for write_text");
            DomainError::ActionFailed {
                reason: format!("spawn clipboard writer: {error}"),
            }
        })?;

        // wl-copy forks a background server and the foreground process exits
        // promptly, so awaiting its status does not block on the selection.
        child.wait().await.map_err(|error| {
            tracing::warn!(%error, "wl-copy did not complete for write_text");
            DomainError::ActionFailed {
                reason: format!("await clipboard writer: {error}"),
            }
        })?;

        Ok(())
    }

    async fn write_bytes(&self, mime: &str, bytes: &[u8]) -> Result<(), DomainError> {
        let mut command = Command::new(&self.program[0]);
        command
            .args(&self.program[1..])
            .arg("--type")
            .arg(mime)
            .stdin(Stdio::piped());

        let mut child = command.spawn().map_err(|error| {
            tracing::warn!(program = %self.program[0], %error, "failed to spawn wl-copy for write_bytes");
            DomainError::ActionFailed {
                reason: format!("spawn clipboard writer: {error}"),
            }
        })?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            tracing::warn!("wl-copy child had no stdin for write_bytes");
            DomainError::ActionFailed {
                reason: "clipboard writer stdin unavailable".to_string(),
            }
        })?;

        stdin.write_all(bytes).await.map_err(|error| {
            tracing::warn!(%error, "failed to write bytes to wl-copy stdin");
            DomainError::ActionFailed {
                reason: format!("write clipboard bytes: {error}"),
            }
        })?;
        // Close stdin so wl-copy sees end-of-input.
        drop(stdin);

        child.wait().await.map_err(|error| {
            tracing::warn!(%error, "wl-copy did not complete for write_bytes");
            DomainError::ActionFailed {
                reason: format!("await clipboard writer: {error}"),
            }
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_text_argv_appends_text() {
        assert_eq!(
            copy_text_argv(&["wl-copy".to_string()], "hi"),
            vec!["wl-copy".to_string(), "hi".to_string()]
        );
    }

    #[test]
    fn copy_text_argv_keeps_fixed_leading_args() {
        assert_eq!(
            copy_text_argv(
                &["wl-copy".to_string(), "--foreground".to_string()],
                "value"
            ),
            vec![
                "wl-copy".to_string(),
                "--foreground".to_string(),
                "value".to_string()
            ]
        );
    }

    #[test]
    fn new_defaults_to_wl_copy_when_none() {
        let writer = WlClipboardWriter::new(None);
        assert_eq!(writer.program(), &["wl-copy".to_string()]);
    }

    #[test]
    fn new_defaults_to_wl_copy_when_empty() {
        let writer = WlClipboardWriter::new(Some(vec![]));
        assert_eq!(writer.program(), &["wl-copy".to_string()]);
    }

    #[test]
    fn new_keeps_supplied_program() {
        let writer = WlClipboardWriter::new(Some(vec![
            "wl-copy".to_string(),
            "--foreground".to_string(),
        ]));
        assert_eq!(
            writer.program(),
            &["wl-copy".to_string(), "--foreground".to_string()]
        );
    }
}
