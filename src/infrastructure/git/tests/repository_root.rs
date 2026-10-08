mod support;

use support::repository;

#[tokio::test]
async fn root_of_a_fresh_repository_resolves() {
    let (_tempdir, root) = repository();

    let resolved = quantum_git::repository_root(root.to_str().expect("utf8 path"))
        .await
        .expect("repository root resolves");

    // Canonicalize both sides: a temporary directory's prefix can itself be a
    // symlink (for example a sandboxed `/tmp`), which git's `--show-toplevel`
    // already resolves but our raw `TempDir` path has not.
    let expected = std::fs::canonicalize(&root).expect("canonicalize root");
    let actual = std::fs::canonicalize(&resolved).expect("canonicalize resolved");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn a_subdirectory_resolves_to_the_same_root() {
    let (_tempdir, root) = repository();
    let subdirectory = root.join("nested").join("deeper");
    std::fs::create_dir_all(&subdirectory).expect("create nested directory");

    let resolved = quantum_git::repository_root(subdirectory.to_str().expect("utf8 path"))
        .await
        .expect("repository root resolves from a subdirectory");

    let expected = std::fs::canonicalize(&root).expect("canonicalize root");
    let actual = std::fs::canonicalize(&resolved).expect("canonicalize resolved");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn a_non_repository_directory_yields_not_a_repository() {
    let tempdir = tempfile::tempdir().expect("create temp directory");
    let directory = tempdir.path().to_str().expect("utf8 path").to_string();

    let error = quantum_git::repository_root(&directory)
        .await
        .expect_err("a plain directory is not a git repository");

    match error {
        quantum_domain::ReviewError::NotARepository(reported) => {
            assert_eq!(reported, directory);
        }
        other => panic!("expected NotARepository, got {other:?}"),
    }
}
