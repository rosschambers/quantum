mod support;

use quantum_domain::RepositoryReview;
use quantum_git::GitRepositoryReview;
use support::{git, repository};

#[tokio::test]
async fn unchanged_repository_fingerprints_equal_across_two_calls() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "modified\n").unwrap();

    let review = GitRepositoryReview;
    let root_str = root.to_str().expect("utf8 path");

    let first = review.fingerprint(root_str).await.expect("fingerprint");
    let second = review.fingerprint(root_str).await.expect("fingerprint");
    assert_eq!(
        first, second,
        "an unchanged repository must fingerprint the same"
    );
}

#[tokio::test]
async fn reediting_an_already_modified_file_changes_the_fingerprint() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "modified once\n").unwrap();

    let review = GitRepositoryReview;
    let root_str = root.to_str().expect("utf8 path");
    let before = review.fingerprint(root_str).await.expect("fingerprint");

    // The path set is unchanged (a.txt was already modified), but its
    // working file metadata (mtime, size) changes with a second edit.
    // Sleep briefly: some filesystems have coarse mtime resolution, and the
    // fingerprint must still change because the edit changes the file size.
    std::thread::sleep(std::time::Duration::from_millis(10));
    std::fs::write(root.join("a.txt"), "modified twice, now longer\n").unwrap();

    let after = review.fingerprint(root_str).await.expect("fingerprint");
    assert_ne!(
        before, after,
        "re-editing an already-modified file must change the fingerprint"
    );
}

#[tokio::test]
async fn creating_an_untracked_file_changes_the_fingerprint() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);

    let review = GitRepositoryReview;
    let root_str = root.to_str().expect("utf8 path");
    let before = review.fingerprint(root_str).await.expect("fingerprint");

    std::fs::write(root.join("new.txt"), "brand new\n").unwrap();
    let after = review.fingerprint(root_str).await.expect("fingerprint");

    assert_ne!(
        before, after,
        "creating an untracked file must change the fingerprint"
    );
}

#[tokio::test]
async fn committing_changes_the_fingerprint() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "modified\n").unwrap();

    let review = GitRepositoryReview;
    let root_str = root.to_str().expect("utf8 path");
    let before = review.fingerprint(root_str).await.expect("fingerprint");

    git(&root, &["commit", "-aq", "-m", "second"]);
    let after = review.fingerprint(root_str).await.expect("fingerprint");

    assert_ne!(
        before, after,
        "committing must change the fingerprint (the path leaves the status list)"
    );
}

#[tokio::test]
async fn staging_a_file_does_not_change_the_fingerprint() {
    let (_tempdir, root) = repository();
    std::fs::write(root.join("a.txt"), "original\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "modified\n").unwrap();

    let review = GitRepositoryReview;
    let root_str = root.to_str().expect("utf8 path");
    let before = review.fingerprint(root_str).await.expect("fingerprint");

    // Staging changes only the index columns of the status line; the path
    // set and the working file are both untouched.
    git(&root, &["add", "a.txt"]);
    let after = review.fingerprint(root_str).await.expect("fingerprint");

    assert_eq!(
        before, after,
        "staging a file must not change the fingerprint, or qv's own Stage \
         button would trigger the changed-on-disk banner"
    );
}
