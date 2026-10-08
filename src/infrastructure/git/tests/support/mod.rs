//! Shared test fixtures for `quantum-git` integration tests: a throwaway git
//! repository in a temporary directory, with local `user.name`/`user.email`
//! so commits never depend on (or disturb) the operator's global git config.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Create a fresh, empty git repository in a new temporary directory.
/// Returns the `TempDir` handle (keep it alive for the test's duration — it
/// deletes the directory on drop) and the repository's root path.
pub fn repository() -> (tempfile::TempDir, PathBuf) {
    let tempdir = tempfile::tempdir().expect("create temp directory");
    let root = tempdir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "Quantum Test"]);
    git(
        &root,
        &["config", "user.email", "quantum-test@example.invalid"],
    );
    (tempdir, root)
}

/// Run a git command against `root` for test fixture setup, panicking with
/// git's stderr on failure. This is scaffolding for building fixtures, not
/// the code under test: the runner module (`run_git`) is exercised directly
/// by the test bodies, not through this helper.
pub fn git(root: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .expect("spawn git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
}
