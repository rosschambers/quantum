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
            Some(
                read_working_tree_side(
                    root_path,
                    &entry.path,
                    &entry.worktree_mode,
                    entry.untracked,
                )
                .await?,
            )
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

/// The all-zero mode/blob sentinels `git diff --raw` uses for "this side does
/// not exist" (mode) and "read this side from the working tree instead of an
/// object" (blob, only ever the dst side, only when no second ref was given).
const ZERO_MODE: &str = "000000";
const ZERO_BLOB: &str = "0000000000000000000000000000000000000000";

/// `changes()` for every `DiffSpec` other than `{ base: "HEAD", target: None
/// }`: a committed base compared against either the working tree (no second
/// ref) or another ref (a ref range such as `qv --diff A..B`). Always
/// read-only (decision D2: staging is only available when comparing HEAD
/// against the working tree, which goes through [`changes_against_head`]).
async fn changes_for_other_base(
    root_path: &Path,
    root: &str,
    spec: &DiffSpec,
) -> Result<ChangeSet, ReviewError> {
    let mut arguments: Vec<&str> = vec!["diff", "--raw", "-z", "-M", "--no-abbrev", &spec.base];
    if let Some(target) = spec.target.as_deref() {
        arguments.push(target);
    }
    let diff_output = run_git(root_path, &arguments, None).await?;
    let mut entries = parse_raw_diff(&diff_output);

    // Only a working-tree comparison (no second ref) can have untracked
    // files: a ref range compares two commits, where nothing is untracked.
    let target_from_disk = spec.target.is_none();
    if target_from_disk {
        let ls_output = run_git(
            root_path,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            None,
        )
        .await?;
        for path in split_nul_strings(&ls_output) {
            entries.push(RawDiffEntry {
                src_mode: ZERO_MODE.to_string(),
                dst_mode: ZERO_MODE.to_string(),
                src_blob: ZERO_BLOB.to_string(),
                dst_blob: ZERO_BLOB.to_string(),
                status: 'A',
                path,
                old_path: None,
                untracked: true,
            });
        }
    }

    // Batch-read every blob this change set needs: the base side always
    // comes from a blob; the target side comes from a blob only when
    // comparing two refs (otherwise it is read from the working tree below).
    let mut blob_ids: Vec<String> = Vec::new();
    for entry in &entries {
        if entry.src_mode != ZERO_MODE {
            blob_ids.push(entry.src_blob.clone());
        }
        if !target_from_disk && entry.dst_mode != ZERO_MODE {
            blob_ids.push(entry.dst_blob.clone());
        }
    }
    let blobs = batch_read_blobs(root_path, &blob_ids).await?;

    let mut files = Vec::with_capacity(entries.len());
    for entry in &entries {
        let base = if entry.src_mode != ZERO_MODE {
            Some(file_side_from_blob(
                &blobs,
                &entry.src_blob,
                &entry.src_mode,
            )?)
        } else {
            None
        };

        let target = if entry.untracked {
            Some(read_working_tree_side(root_path, &entry.path, "", true).await?)
        } else if target_from_disk {
            if entry.dst_mode == ZERO_MODE {
                None
            } else {
                Some(read_working_tree_side(root_path, &entry.path, &entry.dst_mode, false).await?)
            }
        } else if entry.dst_mode != ZERO_MODE {
            Some(file_side_from_blob(
                &blobs,
                &entry.dst_blob,
                &entry.dst_mode,
            )?)
        } else {
            None
        };

        files.push(ChangedFile {
            path: entry.path.clone(),
            old_path: entry.old_path.clone(),
            language: language_for_path(&entry.path),
            base,
            index: None,
            target,
            untracked: entry.untracked,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));

    let target_label = spec
        .target
        .clone()
        .unwrap_or_else(|| "working tree".to_string());

    Ok(ChangeSet {
        repository_root: root.to_string(),
        base_label: spec.base.clone(),
        target_label,
        stageable: false,
        files,
    })
}

/// One entry from `git diff --raw -z -M --no-abbrev`.
struct RawDiffEntry {
    src_mode: String,
    dst_mode: String,
    src_blob: String,
    dst_blob: String,
    /// Kept for parity with the raw record and possible future use (for
    /// example a future "changed/added/deleted" filter); presence/absence
    /// of each side is derived from mode zero-ness instead, so nothing
    /// reads this field today.
    #[allow(dead_code)]
    status: char,
    path: String,
    old_path: Option<String>,
    /// Set only for the synthetic entries `changes_for_other_base` appends
    /// from `git ls-files --others`; never produced by `parse_raw_diff`
    /// itself (`git diff --raw` never reports untracked files).
    untracked: bool,
}

/// Parse `git diff --raw -z -M --no-abbrev` output:
/// `:<srcmode> <dstmode> <srcsha> <dstsha> <status>[score]\0<path>\0`, or for
/// a rename/copy (`status` starts with `R`/`C`),
/// `...\0<srcpath>\0<dstpath>\0` — the OLD path first, then the new one.
fn parse_raw_diff(output: &[u8]) -> Vec<RawDiffEntry> {
    let chunks: Vec<&[u8]> = output
        .split(|&byte| byte == 0)
        .filter(|chunk| !chunk.is_empty())
        .collect();

    let mut entries = Vec::new();
    let mut index = 0;
    while index < chunks.len() {
        let header = String::from_utf8_lossy(chunks[index]);
        let Some(rest) = header.strip_prefix(':') else {
            tracing::warn!(record = %header, "unrecognized git diff --raw record, skipping");
            index += 1;
            continue;
        };

        let mut fields = rest.split(' ');
        let parsed = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        );
        let (Some(src_mode), Some(dst_mode), Some(src_blob), Some(dst_blob), Some(status_field)) =
            parsed
        else {
            tracing::warn!(record = %header, "malformed git diff --raw header, skipping");
            index += 1;
            continue;
        };
        let status = status_field.chars().next().unwrap_or('M');
        let is_rename_or_copy = matches!(status, 'R' | 'C');

        index += 1;
        let first_path = chunks
            .get(index)
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned());

        let (path, old_path) = if is_rename_or_copy {
            index += 1;
            let second_path = chunks
                .get(index)
                .map(|chunk| String::from_utf8_lossy(chunk).into_owned());
            (second_path.unwrap_or_default(), first_path)
        } else {
            (first_path.unwrap_or_default(), None)
        };

        entries.push(RawDiffEntry {
            src_mode: src_mode.to_string(),
            dst_mode: dst_mode.to_string(),
            src_blob: src_blob.to_string(),
            dst_blob: dst_blob.to_string(),
            status,
            path,
            old_path,
            untracked: false,
        });

        index += 1;
    }

    entries
}

/// Split NUL-separated bytes into non-empty UTF-8 strings (used for
/// `git ls-files -z` output, which has no header fields at all).
fn split_nul_strings(output: &[u8]) -> Vec<String> {
    output
        .split(|&byte| byte == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect()
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
///
/// `worktree_mode` is the mode git already knows for a tracked file;
/// `untracked` files have none, so their mode is instead derived from the
/// file's own executable bit (`worktree_mode` is ignored in that case).
async fn read_working_tree_side(
    root_path: &Path,
    path: &str,
    worktree_mode: &str,
    untracked: bool,
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

    let mode = if untracked {
        let executable = metadata.permissions().mode() & 0o111 != 0;
        if executable {
            "100755".to_string()
        } else {
            "100644".to_string()
        }
    } else {
        worktree_mode.to_string()
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
