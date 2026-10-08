//! Runs the system `git` binary as a subprocess (no shell, argv arrays only)
//! and resolves a repository's root directory.

use std::path::Path;
use std::process::Output;
use tokio::process::Command;

use quantum_domain::ReviewError;

/// Run `git` against `root` with `arguments`, optionally piping `stdin` bytes
/// to it, and return its stdout bytes.
///
/// Always passes `-C <root>` so the subprocess's own current directory never
/// matters, sets `GIT_OPTIONAL_LOCKS=0` (so read-only callers such as
/// fingerprint polling never take `index.lock`, which could collide with a
/// concurrent agent's own git commands) and `LC_ALL=C` (so git's messages are
/// in a stable locale the caller can match on, for example the "not a git
/// repository" check in [`repository_root`]). No shell is involved.
///
/// A spawn failure (most commonly `git` missing from quantumd's PATH) maps to
/// [`ReviewError::GitUnavailable`]; a non-zero exit maps to
/// [`ReviewError::GitFailed`] carrying git's trimmed stderr. Every failure
/// path is logged with `tracing::warn!` before returning, so a git failure is
/// never silently swallowed.
pub(crate) async fn run_git(
    root: &Path,
    arguments: &[&str],
    stdin: Option<&[u8]>,
) -> Result<Vec<u8>, ReviewError> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(if stdin.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        });

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            tracing::warn!(%error, ?arguments, "git is not available on quantumd's PATH");
            return Err(ReviewError::GitUnavailable(error.to_string()));
        }
    };

    if let Some(bytes) = stdin {
        use tokio::io::AsyncWriteExt;
        let mut stdin_handle = child
            .stdin
            .take()
            .expect("stdin was requested as piped above");
        if let Err(error) = stdin_handle.write_all(bytes).await {
            tracing::warn!(%error, ?arguments, "failed to write to git's stdin");
            return Err(ReviewError::Io(error.to_string()));
        }
        // Drop explicitly (rather than waiting for scope end) so git sees
        // end-of-input before `wait_with_output` reads its stdout/stderr.
        drop(stdin_handle);
    }

    let output: Output = match child.wait_with_output().await {
        Ok(output) => output,
        Err(error) => {
            tracing::warn!(%error, ?arguments, "failed to wait for git to exit");
            return Err(ReviewError::Io(error.to_string()));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        tracing::warn!(?arguments, %stderr, "git command failed");
        return Err(ReviewError::GitFailed(stderr));
    }

    Ok(output.stdout)
}

/// Resolve the git repository root containing `directory` via
/// `git rev-parse --show-toplevel`.
///
/// A failure whose stderr contains "not a git repository" (the exit-128 case
/// for a plain directory) maps to [`ReviewError::NotARepository`], carrying
/// the directory the caller asked about rather than git's own message.
/// Every other failure (including `git` being absent) passes through
/// unchanged.
pub async fn repository_root(directory: &str) -> Result<String, ReviewError> {
    let root = Path::new(directory);
    match run_git(root, &["rev-parse", "--show-toplevel"], None).await {
        Ok(stdout) => Ok(String::from_utf8_lossy(&stdout).trim().to_string()),
        Err(ReviewError::GitFailed(message)) if message.contains("not a git repository") => {
            Err(ReviewError::NotARepository(directory.to_string()))
        }
        Err(error) => Err(error),
    }
}
