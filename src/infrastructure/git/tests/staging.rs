mod support;

use std::path::Path;
use std::process::Command;

use quantum_domain::{DiffSpec, RepositoryReview, ReviewError};
use quantum_git::GitRepositoryReview;
use support::{git, repository};

fn cached_names(root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--cached", "--name-only"])
        .output()
        .expect("git diff --cached --name-only");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn cached_name_status(root: &Path) -> Vec<(String, String)> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--cached", "--name-status"])
        .output()
        .expect("git diff --cached --name-status");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let status = parts.next().unwrap_or_default().to_string();
            let name = parts.next().unwrap_or_default().to_string();
            (status, name)
        })
        .collect()
}

fn unstaged_names(root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--name-only"])
        .output()
        .expect("git diff --name-only");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn show_index_content(root: &Path, path: &str) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("show")
        .arg(format!(":{path}"))
        .output()
        .expect("git show :<path>");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test]
async fn stage_adds_the_blob_from_changes_to_the_index() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "modified\n").unwrap();

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
        .find(|file| file.path == "a.txt")
        .expect("a.txt in change set");
    let target = file.target.as_ref().expect("target present");

    review
        .stage(
            root.to_str().expect("utf8 path"),
            "a.txt",
            Some(&target.blob),
            &target.mode,
        )
        .await
        .expect("stage succeeds");

    assert_eq!(cached_names(&root), vec!["a.txt".to_string()]);
}

#[tokio::test]
async fn stage_records_exactly_the_blob_that_was_reviewed_even_after_a_later_edit() {
    // D1 proof: Stage must record the content that was DISPLAYED, not
    // whatever is on disk at the moment Stage is clicked.
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "reviewed content\n").unwrap();

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
        .find(|file| file.path == "a.txt")
        .expect("a.txt in change set");
    let target = file.target.as_ref().expect("target present");
    let blob = target.blob.clone();
    let mode = target.mode.clone();

    // The agent keeps editing after the diff was loaded.
    std::fs::write(
        root.join("a.txt"),
        "even newer content, written after the diff loaded\n",
    )
    .unwrap();

    review
        .stage(
            root.to_str().expect("utf8 path"),
            "a.txt",
            Some(&blob),
            &mode,
        )
        .await
        .expect("stage succeeds");

    assert_eq!(
        show_index_content(&root, "a.txt"),
        "reviewed content\n",
        "the index must hold exactly the reviewed content"
    );
    assert!(
        unstaged_names(&root).contains(&"a.txt".to_string()),
        "the post-review edit must show up as a new, unstaged change"
    );
}

#[tokio::test]
async fn stage_an_untracked_file_then_unstage_returns_it_to_untracked() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("new.txt"), "brand new\n").unwrap();

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
        .find(|file| file.path == "new.txt")
        .expect("new.txt in change set");
    let target = file.target.as_ref().expect("target present");

    review
        .stage(
            root.to_str().expect("utf8 path"),
            "new.txt",
            Some(&target.blob),
            &target.mode,
        )
        .await
        .expect("stage succeeds");

    assert!(cached_name_status(&root).contains(&("A".to_string(), "new.txt".to_string())));

    review
        .unstage(root.to_str().expect("utf8 path"), "new.txt")
        .await
        .expect("unstage succeeds");

    let change_set_after = review.changes(&spec).await.expect("changes succeeds");
    let file_after = change_set_after
        .files
        .iter()
        .find(|file| file.path == "new.txt")
        .expect("new.txt still in change set");
    assert!(
        file_after.untracked,
        "unstaging must return it to untracked"
    );
    assert!(file_after.index.is_none());
}

#[tokio::test]
async fn stage_a_deletion_shows_as_a_staged_delete() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("gone.txt"), "will be deleted\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::remove_file(root.join("gone.txt")).unwrap();

    let review = GitRepositoryReview;
    review
        .stage(
            root.to_str().expect("utf8 path"),
            "gone.txt",
            None,
            "100644",
        )
        .await
        .expect("stage succeeds");

    assert!(cached_name_status(&root).contains(&("D".to_string(), "gone.txt".to_string())));
}

#[tokio::test]
async fn unstage_a_staged_modification_returns_it_to_unstaged() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "modified\n").unwrap();
    git(&root, &["add", "a.txt"]);
    assert!(cached_names(&root).contains(&"a.txt".to_string()));

    let review = GitRepositoryReview;
    review
        .unstage(root.to_str().expect("utf8 path"), "a.txt")
        .await
        .expect("unstage succeeds");

    assert!(!cached_names(&root).contains(&"a.txt".to_string()));
    assert!(unstaged_names(&root).contains(&"a.txt".to_string()));
}

const FAKE_BLOB: &str = "0000000000000000000000000000000000000001";

#[tokio::test]
async fn invalid_staging_modes_are_rejected() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "modified\n").unwrap();

    let review = GitRepositoryReview;
    let root_str = root.to_str().expect("utf8 path");

    let error = review
        .stage(root_str, "a.txt", Some(FAKE_BLOB), "100644,evil")
        .await
        .expect_err("a mode with trailing garbage must be rejected");
    assert!(matches!(error, ReviewError::NotStageable(_)));

    let error = review
        .stage(root_str, "a.txt", Some(FAKE_BLOB), "160000")
        .await
        .expect_err("a submodule mode must be rejected");
    assert!(matches!(error, ReviewError::NotStageable(_)));

    assert!(
        cached_names(&root).is_empty(),
        "an invalid mode must never reach git update-index"
    );
}

#[tokio::test]
async fn staging_with_a_repository_root_that_is_not_its_own_toplevel_is_rejected() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "modified\n").unwrap();
    std::fs::create_dir(root.join("subdirectory")).unwrap();

    let review = GitRepositoryReview;
    let root_str = root.to_str().expect("utf8 path");

    let trailing_slash = format!("{root_str}/");
    let error = review
        .stage(&trailing_slash, "a.txt", Some(FAKE_BLOB), "100644")
        .await
        .expect_err("a trailing slash is not the exact toplevel string and must be rejected");
    assert!(matches!(error, ReviewError::NotStageable(_)));

    let subdirectory = format!("{root_str}/subdirectory");
    let error = review
        .stage(&subdirectory, "a.txt", Some(FAKE_BLOB), "100644")
        .await
        .expect_err("a subdirectory resolves to the same toplevel and must be rejected");
    assert!(matches!(error, ReviewError::NotStageable(_)));

    let error = review
        .unstage(&subdirectory, "a.txt")
        .await
        .expect_err("unstage must apply the same repository root check");
    assert!(matches!(error, ReviewError::NotStageable(_)));

    let error = review
        .fingerprint(&subdirectory)
        .await
        .expect_err("fingerprint must apply the same repository root check");
    assert!(matches!(error, ReviewError::NotStageable(_)));

    assert!(
        cached_names(&root).is_empty(),
        "staging must never proceed when the repository root does not match its toplevel"
    );

    // The exact root (no trailing slash, no subdirectory) still works.
    review
        .stage(root_str, "a.txt", Some(FAKE_BLOB), "100644")
        .await
        .expect("the exact repository root must still be accepted");
}

#[tokio::test]
async fn escaping_paths_are_rejected_for_both_stage_and_unstage() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);

    let review = GitRepositoryReview;
    let root_str = root.to_str().expect("utf8 path");

    let error = review
        .stage(root_str, "../escape", Some("abc"), "100644")
        .await
        .expect_err("a parent-dir path must be rejected");
    assert!(matches!(error, ReviewError::NotStageable(_)));

    let error = review
        .stage(root_str, "/etc/passwd", Some("abc"), "100644")
        .await
        .expect_err("an absolute path must be rejected");
    assert!(matches!(error, ReviewError::NotStageable(_)));

    let error = review
        .unstage(root_str, "../escape")
        .await
        .expect_err("a parent-dir path must be rejected on unstage too");
    assert!(matches!(error, ReviewError::NotStageable(_)));

    let error = review
        .unstage(root_str, "/etc/passwd")
        .await
        .expect_err("an absolute path must be rejected on unstage too");
    assert!(matches!(error, ReviewError::NotStageable(_)));
}
