//! C1: a symlink in a reviewed repository must never be dereferenced. Its
//! "content" is the link target string itself (what `readlink` returns),
//! never the bytes of whatever file the link points at — matching git's own
//! model of a symlink as a blob whose content is the target path.

mod support;

use std::path::Path;
use std::process::Command;

use quantum_domain::{DiffSpec, RepositoryReview};
use quantum_git::GitRepositoryReview;
use support::{git, repository};

/// A marker unique enough that finding it anywhere (a diff's content, a
/// repository's object database) proves the outside file's bytes leaked in.
const MARKER: &str = "OUTSIDE-SECRET-MARKER-f3b9c2d1";

fn hash_object_stdin(root: &Path, bytes: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["hash-object", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn git hash-object --stdin");
    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(bytes)
        .expect("write link target bytes");
    let output = child.wait_with_output().expect("wait for git hash-object");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn cat_file_pretty(root: &Path, revision: &str) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-p", revision])
        .output()
        .expect("spawn git cat-file -p");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn ls_files_stage(root: &Path, path: &str) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-s", "--", path])
        .output()
        .expect("spawn git ls-files -s");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Every object currently in the repository's object database, as raw
/// bytes, via `git cat-file --batch-all-objects --batch`. Used to prove the
/// outside file's marker was never written into this repository's object
/// store.
fn all_object_bytes(root: &Path) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch-all-objects", "--batch"])
        .output()
        .expect("spawn git cat-file --batch-all-objects --batch");
    assert!(output.status.success());
    output.stdout
}

#[tokio::test]
async fn a_tracked_symlink_to_an_outside_file_reports_the_link_target_not_the_outside_content() {
    let (_tempdir, root) = repository();
    let (_outside_tempdir, outside_root) = repository();
    let outside_file = outside_root.join("secret.txt");
    std::fs::write(&outside_file, MARKER).unwrap();

    std::fs::write(root.join("a.txt"), "first\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);

    let target = outside_file.to_str().expect("utf8 path").to_string();
    std::os::unix::fs::symlink(&target, root.join("link")).expect("create symlink");
    // Staged, not yet committed: this is the "tracked" case (git already
    // knows about it as an indexed addition), exercising the target-side
    // read of a mode-120000 entry.
    git(&root, &["add", "link"]);

    let review = GitRepositoryReview;
    let spec = DiffSpec {
        repository: root.to_str().expect("utf8 path").to_string(),
        base: "HEAD".to_string(),
        target: None,
    };
    let change_set = review.changes(&spec).await.expect("changes succeeds");
    let file = change_set
        .files
        .iter()
        .find(|file| file.path == "link")
        .expect("link in change set");
    let side = file.target.as_ref().expect("target present");

    assert_eq!(
        side.mode, "120000",
        "a symlink must be reported as mode 120000"
    );
    assert_eq!(
        side.content.as_deref(),
        Some(target.as_str()),
        "content must be the link target path string, never the file it points at"
    );
    assert!(
        !side.content.as_deref().unwrap_or_default().contains(MARKER),
        "the outside file's content must never appear in the reported content"
    );

    let expected_blob = hash_object_stdin(&root, target.as_bytes());
    assert_eq!(
        side.blob, expected_blob,
        "the blob id must hash the link target bytes, never the file it points at"
    );
}

#[tokio::test]
async fn an_untracked_symlink_to_an_outside_file_reports_the_link_target_not_the_outside_content() {
    let (_tempdir, root) = repository();
    let (_outside_tempdir, outside_root) = repository();
    let outside_file = outside_root.join("secret.txt");
    std::fs::write(&outside_file, MARKER).unwrap();

    std::fs::write(root.join("a.txt"), "first\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);

    let target = outside_file.to_str().expect("utf8 path").to_string();
    std::os::unix::fs::symlink(&target, root.join("link")).expect("create symlink");
    // Never `git add`ed: this is the untracked case.

    let review = GitRepositoryReview;
    let spec = DiffSpec {
        repository: root.to_str().expect("utf8 path").to_string(),
        base: "HEAD".to_string(),
        target: None,
    };
    let change_set = review.changes(&spec).await.expect("changes succeeds");
    let file = change_set
        .files
        .iter()
        .find(|file| file.path == "link")
        .expect("link in change set");
    assert!(file.untracked);
    let side = file.target.as_ref().expect("target present");

    assert_eq!(
        side.mode, "120000",
        "an untracked symlink must still be reported as mode 120000"
    );
    assert_eq!(side.content.as_deref(), Some(target.as_str()));
    assert!(!side.content.as_deref().unwrap_or_default().contains(MARKER));

    let expected_blob = hash_object_stdin(&root, target.as_bytes());
    assert_eq!(side.blob, expected_blob);
}

#[tokio::test]
async fn staging_a_symlink_writes_mode_120000_with_the_link_target_as_content_and_leaks_nothing() {
    let (_tempdir, root) = repository();
    let (_outside_tempdir, outside_root) = repository();
    let outside_file = outside_root.join("secret.txt");
    std::fs::write(&outside_file, MARKER).unwrap();

    std::fs::write(root.join("a.txt"), "first\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);

    let target = outside_file.to_str().expect("utf8 path").to_string();
    std::os::unix::fs::symlink(&target, root.join("link")).expect("create symlink");

    let review = GitRepositoryReview;
    let spec = DiffSpec {
        repository: root.to_str().expect("utf8 path").to_string(),
        base: "HEAD".to_string(),
        target: None,
    };
    let change_set = review.changes(&spec).await.expect("changes succeeds");
    let file = change_set
        .files
        .iter()
        .find(|file| file.path == "link")
        .expect("link in change set");
    let side = file.target.as_ref().expect("target present");
    let blob = side.blob.clone();
    let mode = side.mode.clone();

    review
        .stage(
            root.to_str().expect("utf8 path"),
            "link",
            Some(&blob),
            &mode,
        )
        .await
        .expect("stage succeeds");

    let staged = ls_files_stage(&root, "link");
    assert!(
        staged.starts_with("120000"),
        "git ls-files -s must report mode 120000 for the staged symlink, got: {staged}"
    );

    let staged_content = cat_file_pretty(&root, ":link");
    assert_eq!(
        staged_content, target,
        "the staged blob must equal the link target path string"
    );

    let objects = all_object_bytes(&root);
    assert!(
        !objects
            .windows(MARKER.len())
            .any(|window| window == MARKER.as_bytes()),
        "no object in the repository must ever contain the outside file's content"
    );
}
