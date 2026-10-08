//! `RepositoryReview` domain port implementation, backed by the git CLI.

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quantum_domain::{
    is_likely_binary, language_for_extension, ChangeSet, ChangedFile, DiffSpec, FileSide,
    RepositoryReview, ReviewError, VIEWER_TEXT_MAX_BYTES,
};

use crate::contents::batch_read_blobs;
use crate::runner::{repository_root, run_git};
use crate::status::{parse_status, StatusEntry};

/// Git-backed implementation of the `RepositoryReview` domain port.
/// Stateless: every call resolves the repository root fresh and shells out
/// to `git`, so there is nothing to keep consistent between calls here (the
/// application-layer `ReviewService` is where per-window state, like the
/// staging safety boundary, lives).
pub struct GitRepositoryReview;

#[async_trait]
impl RepositoryReview for GitRepositoryReview {
    async fn changes(&self, spec: &DiffSpec) -> Result<ChangeSet, ReviewError> {
        let root = repository_root(&spec.repository).await?;
        let root_path = PathBuf::from(&root);

        if spec.base == "HEAD" && spec.target.is_none() {
            changes_against_head(&root_path, &root).await
        } else {
            changes_for_other_base(&root_path, &root, spec).await
        }
    }

    async fn stage(
        &self,
        _repository_root: &str,
        _path: &str,
        _blob: Option<&str>,
        _mode: &str,
    ) -> Result<(), ReviewError> {
        Err(ReviewError::GitFailed(
            "stage is not yet implemented".to_string(),
        ))
    }

    async fn unstage(&self, _repository_root: &str, _path: &str) -> Result<(), ReviewError> {
        Err(ReviewError::GitFailed(
            "unstage is not yet implemented".to_string(),
        ))
    }

    async fn fingerprint(&self, _repository_root: &str) -> Result<String, ReviewError> {
        Err(ReviewError::GitFailed(
            "fingerprint is not yet implemented".to_string(),
        ))
    }
}

/// `changes()` for `DiffSpec { base: "HEAD", target: None }`: one
/// `git status --porcelain=v2` call reports staged, unstaged, and untracked
/// changes together, and the change set is stageable.
async fn changes_against_head(root_path: &Path, root: &str) -> Result<ChangeSet, ReviewError> {
    let status_output = run_git(
        root_path,
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        None,
    )
    .await?;
    let entries = parse_status(&status_output);

    // One batched `git cat-file --batch` call for every blob this change set
    // needs, rather than one process per file.
    let mut blob_ids: Vec<String> = Vec::new();
    for entry in &entries {
        if base_present(entry) {
            blob_ids.push(entry.head_blob.clone());
        }
        if index_present(entry) {
            blob_ids.push(entry.index_blob.clone());
        }
    }
    let blobs = batch_read_blobs(root_path, &blob_ids).await?;

    let mut files = Vec::with_capacity(entries.len());
    for entry in &entries {
        let base = if base_present(entry) {
            Some(file_side_from_blob(
                &blobs,
                &entry.head_blob,
                &entry.head_mode,
            )?)
        } else {
            None
        };
        let index = if index_present(entry) {
            Some(file_side_from_blob(
                &blobs,
                &entry.index_blob,
                &entry.index_mode,
            )?)
        } else {
            None
        };
        let target = if target_present(entry) {
            Some(read_working_tree_side(root_path, &entry.path, entry).await?)
        } else {
            None
        };

        files.push(ChangedFile {
            path: entry.path.clone(),
            old_path: entry.old_path.clone(),
            language: language_for_path(&entry.path),
            base,
            index,
            target,
            untracked: entry.untracked,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(ChangeSet {
        repository_root: root.to_string(),
        base_label: "HEAD".to_string(),
        target_label: "working tree".to_string(),
        stageable: true,
        files,
    })
}

/// `changes()` for every `DiffSpec` other than `{ base: "HEAD", target: None
/// }`: implemented in a later commit (arbitrary refs and ref ranges, always
/// read-only).
async fn changes_for_other_base(
    _root_path: &Path,
    _root: &str,
    _spec: &DiffSpec,
) -> Result<ChangeSet, ReviewError> {
    Err(ReviewError::GitFailed(
        "comparing against a base other than HEAD is not yet implemented".to_string(),
    ))
}

/// Whether a status entry has a HEAD-side file: untracked files and newly
/// added files (`head_mode == "000000"`) have none.
fn base_present(entry: &StatusEntry) -> bool {
    !entry.untracked && entry.head_mode != "000000"
}

/// Whether a status entry has an index-side file: untracked files and
/// staged deletions (`index_mode == "000000"`) have none.
fn index_present(entry: &StatusEntry) -> bool {
    !entry.untracked && entry.index_mode != "000000"
}

/// Whether a status entry has a working-tree file to read: untracked files
/// always do (that is what "untracked" means); tracked files do unless the
/// working tree copy was deleted (`worktree_mode == "000000"`).
fn target_present(entry: &StatusEntry) -> bool {
    entry.untracked || entry.worktree_mode != "000000"
}

fn language_for_path(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(language_for_extension)
}

/// Build a `FileSide` for a blob side (base or index) from bytes already
/// fetched by the batched `cat-file --batch` call. `binary`/`too_large`
/// follow the same rules as the working-tree side.
fn file_side_from_blob(
    blobs: &HashMap<String, Vec<u8>>,
    blob_id: &str,
    mode: &str,
) -> Result<FileSide, ReviewError> {
    let bytes = blobs.get(blob_id).ok_or_else(|| {
        ReviewError::GitFailed(format!("blob {blob_id} missing from cat-file batch"))
    })?;

    let too_large = bytes.len() as u64 > VIEWER_TEXT_MAX_BYTES;
    let mut binary = false;
    let mut content = None;
    if !too_large {
        if is_likely_binary(bytes) {
            binary = true;
        } else {
            match std::str::from_utf8(bytes) {
                Ok(text) => content = Some(text.to_string()),
                Err(_) => binary = true,
            }
        }
    }

    Ok(FileSide {
        content,
        blob: blob_id.to_string(),
        mode: mode.to_string(),
        binary,
        too_large,
    })
}

/// Build the working-tree `FileSide` by reading the file from disk and
/// writing its exact current bytes into the object database (decision D1:
/// Stage must stage exactly what was displayed, even if the file changes
/// again before the user clicks Stage).
async fn read_working_tree_side(
    root_path: &Path,
    path: &str,
    entry: &StatusEntry,
) -> Result<FileSide, ReviewError> {
    let file_path = root_path.join(path);

    let metadata = tokio::fs::metadata(&file_path).await.map_err(|error| {
        tracing::warn!(path = %file_path.display(), %error, "failed to stat working tree file");
        ReviewError::Io(error.to_string())
    })?;
    let size = metadata.len();
    let too_large = size > VIEWER_TEXT_MAX_BYTES;

    let mut binary = false;
    let mut content = None;
    if !too_large {
        let bytes = tokio::fs::read(&file_path).await.map_err(|error| {
            tracing::warn!(path = %file_path.display(), %error, "failed to read working tree file");
            ReviewError::Io(error.to_string())
        })?;
        if is_likely_binary(&bytes) {
            binary = true;
        } else {
            match String::from_utf8(bytes) {
                Ok(text) => content = Some(text),
                Err(_) => binary = true,
            }
        }
    }

    let mode = if entry.untracked {
        let executable = metadata.permissions().mode() & 0o111 != 0;
        if executable {
            "100755".to_string()
        } else {
            "100644".to_string()
        }
    } else {
        entry.worktree_mode.clone()
    };

    let path_argument = format!("--path={path}");
    let file_path_argument = file_path.to_string_lossy().into_owned();
    let hash_output = run_git(
        root_path,
        &[
            "hash-object",
            "-w",
            &path_argument,
            "--",
            &file_path_argument,
        ],
        None,
    )
    .await?;
    let blob = String::from_utf8_lossy(&hash_output).trim().to_string();

    Ok(FileSide {
        content,
        blob,
        mode,
        binary,
        too_large,
    })
}
