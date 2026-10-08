mod support;

use quantum_domain::{ChangeSet, ChangedFile, DiffSpec, RepositoryReview};
use quantum_git::GitRepositoryReview;
use support::{git, repository};

fn find<'a>(change_set: &'a ChangeSet, path: &str) -> &'a ChangedFile {
    change_set
        .files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| {
            panic!(
                "file {path} not found in change set: {:?}",
                change_set.files
            )
        })
}

fn hash_object(root: &std::path::Path, path: &str) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("hash-object")
        .arg(path)
        .output()
        .expect("spawn git hash-object");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[tokio::test]
async fn reports_every_kind_of_change_against_head() {
    let (_tempdir, root) = repository();

    // Initial commit with several tracked files AND the .gitignore rule.
    // Committing .gitignore together with everything else (rather than in a
    // later, separate commit) matters: `git commit` commits the whole
    // index, so a later commit would also sweep in whatever had already
    // been `git add`ed for staged.txt/partial.txt below, defeating the
    // staged/partially-staged scenarios this test is trying to set up.
    std::fs::write(root.join("modified.txt"), "original\n").unwrap();
    std::fs::write(root.join("staged.txt"), "original\n").unwrap();
    std::fs::write(root.join("partial.txt"), "original\n").unwrap();
    std::fs::write(root.join("deleted.txt"), "original\n").unwrap();
    std::fs::write(root.join("renamed-old.txt"), "original\n").unwrap();
    std::fs::write(root.join("binary.bin"), vec![0u8, 1, 2, 3, 0]).unwrap();
    std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);

    // Modify a file, unstaged.
    std::fs::write(root.join("modified.txt"), "modified unstaged\n").unwrap();

    // Stage another file fully (no further edits after staging).
    std::fs::write(root.join("staged.txt"), "staged content\n").unwrap();
    git(&root, &["add", "staged.txt"]);

    // Partially stage a third: stage once, then edit again.
    std::fs::write(root.join("partial.txt"), "staged part\n").unwrap();
    git(&root, &["add", "partial.txt"]);
    std::fs::write(root.join("partial.txt"), "staged part, then edited again\n").unwrap();

    // Untracked file.
    std::fs::write(root.join("untracked.txt"), "brand new\n").unwrap();

    // An ignored file must never appear, even though it exists on disk.
    std::fs::write(root.join("ignored.txt"), "should never show up\n").unwrap();

    // Delete a tracked file without staging the deletion.
    std::fs::remove_file(root.join("deleted.txt")).unwrap();

    // Rename a tracked file (git detects it as a pure rename).
    git(&root, &["mv", "renamed-old.txt", "renamed-new.txt"]);

    // Change the committed binary file on disk.
    std::fs::write(root.join("binary.bin"), vec![9u8, 8, 7, 0, 6]).unwrap();

    // A large untracked file, over the viewer size cap (5 MiB).
    let large_bytes = vec![b'a'; 6 * 1024 * 1024];
    std::fs::write(root.join("large.bin"), &large_bytes).unwrap();

    let review = GitRepositoryReview;
    let spec = DiffSpec {
        repository: root.to_str().expect("utf8 path").to_string(),
        base: "HEAD".to_string(),
        target: None,
    };
    let change_set = review.changes(&spec).await.expect("changes succeeds");

    assert!(change_set.stageable);
    assert_eq!(change_set.base_label, "HEAD");
    assert_eq!(change_set.target_label, "working tree");

    // The ignored file never appears, despite existing on disk.
    assert!(!change_set
        .files
        .iter()
        .any(|file| file.path == "ignored.txt"));

    // Modified, unstaged only: index is unchanged from base (same blob);
    // target differs from both.
    let modified = find(&change_set, "modified.txt");
    let modified_base = modified.base.as_ref().expect("base present");
    let modified_index = modified.index.as_ref().expect("index present");
    assert_eq!(modified_base.blob, modified_index.blob);
    assert_eq!(
        modified.target.as_ref().expect("target present").content,
        Some("modified unstaged\n".to_string())
    );

    // Fully staged, no further edits: index differs from base; target
    // equals index.
    let staged = find(&change_set, "staged.txt");
    let staged_base = staged.base.as_ref().expect("base present");
    let staged_index = staged.index.as_ref().expect("index present");
    let staged_target = staged.target.as_ref().expect("target present");
    assert_ne!(staged_base.blob, staged_index.blob);
    assert_eq!(staged_index.blob, staged_target.blob);
    assert_eq!(staged_target.content, Some("staged content\n".to_string()));

    // Partially staged: index differs from base AND target differs from
    // index (the "partially staged" case).
    let partial = find(&change_set, "partial.txt");
    let partial_base = partial.base.as_ref().expect("base present");
    let partial_index = partial.index.as_ref().expect("index present");
    let partial_target = partial.target.as_ref().expect("target present");
    assert_ne!(partial_base.blob, partial_index.blob);
    assert_ne!(partial_index.blob, partial_target.blob);
    assert_eq!(
        partial_target.content,
        Some("staged part, then edited again\n".to_string())
    );

    // Untracked: no base, no index; target present and flagged untracked.
    let untracked = find(&change_set, "untracked.txt");
    assert!(untracked.base.is_none());
    assert!(untracked.index.is_none());
    assert!(untracked.untracked);
    assert_eq!(
        untracked.target.as_ref().expect("target present").content,
        Some("brand new\n".to_string())
    );

    // Deleted (unstaged): base present, target absent.
    let deleted = find(&change_set, "deleted.txt");
    assert!(deleted.base.is_some());
    assert!(deleted.target.is_none());

    // Renamed: path is the new name, old_path records the original.
    let renamed = find(&change_set, "renamed-new.txt");
    assert_eq!(renamed.old_path.as_deref(), Some("renamed-old.txt"));
    assert!(!change_set
        .files
        .iter()
        .any(|file| file.path == "renamed-old.txt"));

    // Binary file: both the committed base and the changed target are
    // reported as binary with no content.
    let binary = find(&change_set, "binary.bin");
    let binary_base = binary.base.as_ref().expect("base present");
    assert!(binary_base.binary);
    assert!(binary_base.content.is_none());
    let binary_target = binary.target.as_ref().expect("target present");
    assert!(binary_target.binary);
    assert!(binary_target.content.is_none());

    // Large file: too_large on the target side, with no content, but still
    // a real, working blob id (decision D1: stage what was displayed).
    let large = find(&change_set, "large.bin");
    assert!(large.base.is_none());
    let large_target = large.target.as_ref().expect("target present");
    assert!(large_target.too_large);
    assert!(large_target.content.is_none());

    // D1 groundwork: the working-tree blob id for several files equals
    // `git hash-object` of that file's current on-disk content.
    for path in ["modified.txt", "staged.txt", "large.bin"] {
        let changed_file = find(&change_set, path);
        let expected_blob = hash_object(&root, path);
        assert_eq!(
            changed_file.target.as_ref().expect("target present").blob,
            expected_blob,
            "{path}: working tree blob id must equal git hash-object"
        );
    }
}

#[tokio::test]
async fn base_other_than_head_against_working_tree_is_read_only_and_sees_both_layers() {
    let (_tempdir, root) = repository();

    std::fs::write(root.join("a.txt"), "first\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "first commit"]);

    // A second commit, so HEAD~1 is one commit behind HEAD.
    std::fs::write(root.join("a.txt"), "second\n").unwrap();
    git(&root, &["commit", "-aq", "-m", "second commit"]);

    // An uncommitted edit on top of HEAD, so the working tree differs from
    // both HEAD and HEAD~1.
    std::fs::write(root.join("a.txt"), "second, uncommitted\n").unwrap();

    let review = GitRepositoryReview;
    let spec = DiffSpec {
        repository: root.to_str().expect("utf8 path").to_string(),
        base: "HEAD~1".to_string(),
        target: None,
    };
    let change_set = review.changes(&spec).await.expect("changes succeeds");

    assert!(
        !change_set.stageable,
        "a base other than HEAD must never be stageable"
    );
    assert_eq!(change_set.base_label, "HEAD~1");
    assert_eq!(change_set.target_label, "working tree");

    let file = find(&change_set, "a.txt");
    assert!(file.index.is_none(), "non-HEAD bases report no index side");
    assert_eq!(
        file.base.as_ref().expect("base present").content,
        Some("first\n".to_string()),
        "base must be HEAD~1's committed content, not HEAD's"
    );
    assert_eq!(
        file.target.as_ref().expect("target present").content,
        Some("second, uncommitted\n".to_string()),
        "target must be the uncommitted working tree content, not HEAD's"
    );
}

#[tokio::test]
async fn ref_range_shows_only_the_committed_difference() {
    let (_tempdir, root) = repository();

    std::fs::write(root.join("a.txt"), "first\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "first commit"]);

    std::fs::write(root.join("a.txt"), "second\n").unwrap();
    git(&root, &["commit", "-aq", "-m", "second commit"]);

    // An uncommitted edit that must NOT show up in a ref-range comparison:
    // a ref range compares two commits only.
    std::fs::write(root.join("a.txt"), "uncommitted, must not appear\n").unwrap();

    let review = GitRepositoryReview;
    let spec = DiffSpec {
        repository: root.to_str().expect("utf8 path").to_string(),
        base: "HEAD~1".to_string(),
        target: Some("HEAD".to_string()),
    };
    let change_set = review.changes(&spec).await.expect("changes succeeds");

    assert!(!change_set.stageable);
    assert_eq!(change_set.base_label, "HEAD~1");
    assert_eq!(change_set.target_label, "HEAD");

    let file = find(&change_set, "a.txt");
    assert_eq!(
        file.base.as_ref().expect("base present").content,
        Some("first\n".to_string())
    );
    assert_eq!(
        file.target.as_ref().expect("target present").content,
        Some("second\n".to_string()),
        "target must be HEAD's committed content, not the uncommitted working tree edit"
    );
}

fn rev_parse(root: &std::path::Path, reference: &str) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("rev-parse")
        .arg(reference)
        .output()
        .expect("spawn git rev-parse");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[tokio::test]
async fn an_oversized_committed_blob_is_reported_too_large_without_reading_its_full_content() {
    let (_tempdir, root) = repository();

    // A 6 MiB committed file, over the 5 MiB viewer cap, then modified in
    // the working tree so the BASE side goes through the committed-blob
    // (`cat-file --batch`) path this test targets.
    let large_bytes = vec![b'a'; 6 * 1024 * 1024];
    std::fs::write(root.join("large.bin"), &large_bytes).unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("large.bin"), vec![b'b'; 6 * 1024 * 1024]).unwrap();

    let review = GitRepositoryReview;
    let spec = DiffSpec {
        repository: root.to_str().expect("utf8 path").to_string(),
        base: "HEAD".to_string(),
        target: None,
    };
    let change_set = review.changes(&spec).await.expect("changes succeeds");

    let large = find(&change_set, "large.bin");
    let base = large.base.as_ref().expect("base present");
    assert!(
        base.too_large,
        "a committed blob over the size cap must be reported too_large"
    );
    assert!(
        base.content.is_none(),
        "an oversized committed blob's content must never be read into memory"
    );
}

#[tokio::test]
async fn submodule_gitlink_is_never_rendered_as_file_content() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "first\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);

    // A real nested repository checked out at "sub", recorded as a gitlink
    // (mode 160000) the way a git submodule would be -- no `.gitmodules`
    // file is needed; the status/diff machinery only cares about the mode.
    let sub_root = root.join("sub");
    std::fs::create_dir(&sub_root).unwrap();
    git(&sub_root, &["init", "-q"]);
    git(&sub_root, &["config", "user.name", "Quantum Test"]);
    git(
        &sub_root,
        &["config", "user.email", "quantum-test@example.invalid"],
    );
    std::fs::write(sub_root.join("f.txt"), "submodule content\n").unwrap();
    git(&sub_root, &["add", "."]);
    git(&sub_root, &["commit", "-q", "-m", "submodule initial"]);
    let sub_commit = rev_parse(&sub_root, "HEAD");

    git(
        &root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{sub_commit},sub"),
        ],
    );
    git(&root, &["commit", "-q", "-m", "add gitlink"]);

    // Dirty the submodule's working content without changing its pinned
    // commit, so the gitlink entry's WORKING TREE side also reports mode
    // 160000 (exercising the working-tree guard, not just the
    // committed-blob guard).
    std::fs::write(sub_root.join("f.txt"), "submodule content, dirtied\n").unwrap();

    let review = GitRepositoryReview;
    let spec = DiffSpec {
        repository: root.to_str().expect("utf8 path").to_string(),
        base: "HEAD".to_string(),
        target: None,
    };
    let change_set = review
        .changes(&spec)
        .await
        .expect("changes succeeds even with a submodule gitlink present");

    let sub_file = find(&change_set, "sub");
    let base = sub_file.base.as_ref().expect("base present");
    assert!(
        base.binary,
        "a gitlink's base side must never be rendered as text content"
    );
    assert!(base.content.is_none());

    let index = sub_file.index.as_ref().expect("index present");
    assert!(index.binary);
    assert!(index.content.is_none());

    let target = sub_file.target.as_ref().expect("target present");
    assert!(
        target.binary,
        "a gitlink's working-tree side must never be rendered as text content"
    );
    assert!(target.content.is_none());

    for side in [base, index, target] {
        if let Some(content) = &side.content {
            assert!(
                !content.contains("submodule content"),
                "the submodule's own file content must never leak into any side"
            );
        }
    }
}

#[tokio::test]
async fn an_unknown_ref_surfaces_gits_error() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "first\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "first commit"]);

    let review = GitRepositoryReview;
    let spec = DiffSpec {
        repository: root.to_str().expect("utf8 path").to_string(),
        base: "not-a-real-ref".to_string(),
        target: None,
    };
    let error = review
        .changes(&spec)
        .await
        .expect_err("an unknown ref must fail");

    match error {
        quantum_domain::ReviewError::GitFailed(message) => {
            assert!(!message.is_empty(), "the error must carry git's own stderr");
        }
        other => panic!("expected GitFailed, got {other:?}"),
    }
}
