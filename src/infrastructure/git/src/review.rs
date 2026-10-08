//! `RepositoryReview` domain port implementation, backed by the git CLI.

use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quantum_domain::{
    is_likely_binary, language_for_extension, ChangeSet, ChangedFile, DiffSpec, FileSide,
    RepositoryReview, ReviewError, VIEWER_TEXT_MAX_BYTES,
};

use crate::contents::{batch_read_blobs, BlobEntry};
use crate::runner::{repository_root as resolve_repository_root, run_git};
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
        let root = resolve_repository_root(&spec.repository).await?;
        let root_path = PathBuf::from(&root);

        if spec.base == "HEAD" && spec.target.is_none() {
            changes_against_head(&root_path, &root).await
        } else {
            changes_for_other_base(&root_path, &root, spec).await
        }
    }

    async fn stage(
        &self,
        repository_root: &str,
        path: &str,
        blob: Option<&str>,
        mode: &str,
    ) -> Result<(), ReviewError> {
        if blob.is_some() {
            validate_mode(mode)?;
        }
        validate_relative_path(path)?;
        verify_repository_root(repository_root).await?;
        let root_path = Path::new(repository_root);

        match blob {
            // Decision D1: stage exactly `blob`, regardless of what is on
            // disk right now. `--cacheinfo <mode>,<blob>,<path>` writes that
            // triple into the index without touching the working tree.
            Some(blob_id) => {
                let cacheinfo = format!("{mode},{blob_id},{path}");
                run_git(
                    root_path,
                    &["update-index", "--add", "--cacheinfo", &cacheinfo],
                    None,
                )
                .await?;
            }
            // `blob: None` stages a deletion: remove the path from the index
            // without requiring it to still exist on disk (`--force-remove`).
            None => {
                run_git(
                    root_path,
                    &["update-index", "--force-remove", "--", path],
                    None,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn unstage(&self, repository_root: &str, path: &str) -> Result<(), ReviewError> {
        validate_relative_path(path)?;
        verify_repository_root(repository_root).await?;
        let root_path = Path::new(repository_root);

        // A path staged as a brand-new file has no HEAD counterpart to
        // restore the index entry from; `git restore --staged` would error
        // on it, so such a path is unstaged with `git rm --cached` instead,
        // returning it to untracked.
        let head_reference = format!("HEAD:{path}");
        let exists_in_head = run_git(root_path, &["cat-file", "-e", &head_reference], None)
            .await
            .is_ok();

        if exists_in_head {
            run_git(root_path, &["restore", "--staged", "--", path], None).await?;
        } else {
            run_git(root_path, &["rm", "--cached", "-q", "--", path], None).await?;
        }

        Ok(())
    }

    /// A cheap value that changes whenever the working tree or index
    /// changes, for the "files changed on disk" banner's polling loop.
    ///
    /// Deliberately hashes the SORTED SET OF PATHS `git status` lists,
    /// each followed by its working file's `(modified time in nanoseconds,
    /// size)`, and nothing else — never the status letters (`index_status`/
    /// `worktree_status`). Staging a file changes only the index columns of
    /// its status line, never the path set or the working file itself, so
    /// qv's own Stage/Unstage buttons must never flip this fingerprint
    /// (which would show the "changed on disk" banner after the user's own
    /// click). A missing working file (for example a staged deletion with
    /// no working-tree copy) contributes a fixed marker instead of
    /// metadata, so its absence still affects the hash deterministically.
    ///
    /// Accepted limitation: an external `git add` that changes no working
    /// file (for example staging a rename with identical content) changes
    /// neither the path set nor any working file's metadata, so it is not
    /// detected until the next manual refresh.
    async fn fingerprint(&self, repository_root: &str) -> Result<String, ReviewError> {
        verify_repository_root(repository_root).await?;
        let root_path = Path::new(repository_root);
        let status_output = run_git(
            root_path,
            &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
            None,
        )
        .await?;
        let entries = parse_status(&status_output);

        let mut paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        paths.sort_unstable();

        let mut hash = FNV_OFFSET_BASIS;
        for path in paths {
            hash = fnv1a_update(hash, path.as_bytes());
            hash = fnv1a_update(hash, &[0]);

            match tokio::fs::symlink_metadata(root_path.join(path)).await {
                Ok(metadata) => {
                    let modified_nanos = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| duration.as_nanos())
                        .unwrap_or(0);
                    hash = fnv1a_update(hash, &modified_nanos.to_le_bytes());
                    hash = fnv1a_update(hash, &metadata.len().to_le_bytes());
                }
                Err(_) => {
                    hash = fnv1a_update(hash, b"missing");
                }
            }
            hash = fnv1a_update(hash, &[0]);
        }

        Ok(format!("{hash:016x}"))
    }
}

/// FNV-1a 64-bit hash, implemented inline (no new dependency) for
/// [`GitRepositoryReview::fingerprint`]. See
/// <http://www.isthe.com/chongo/tech/comp/fnv/> for the algorithm.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn fnv1a_update(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
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
    // needs, rather than one process per file. A gitlink's "blob" id
    // actually names a commit object (M1: submodules), so it is excluded
    // here and never fetched at all.
    let mut blob_ids: Vec<String> = Vec::new();
    for entry in &entries {
        if base_present(entry) && !is_gitlink_mode(&entry.head_mode) {
            blob_ids.push(entry.head_blob.clone());
        }
        if index_present(entry) && !is_gitlink_mode(&entry.index_mode) {
            blob_ids.push(entry.index_blob.clone());
        }
    }
    let blobs = batch_read_blobs(root_path, &blob_ids).await?;

    let mut files = Vec::with_capacity(entries.len());
    for entry in &entries {
        let base = if base_present(entry) {
            Some(if is_gitlink_mode(&entry.head_mode) {
                gitlink_side(&entry.path, &entry.head_blob)
            } else {
                file_side_from_blob(&blobs, &entry.head_blob, &entry.head_mode)?
            })
        } else {
            None
        };
        let index = if index_present(entry) {
            Some(if is_gitlink_mode(&entry.index_mode) {
                gitlink_side(&entry.path, &entry.index_blob)
            } else {
                file_side_from_blob(&blobs, &entry.index_blob, &entry.index_mode)?
            })
        } else {
            None
        };
        let target = if target_present(entry) {
            Some(if is_gitlink_mode(&entry.worktree_mode) {
                gitlink_side(&entry.path, gitlink_reference(entry))
            } else {
                read_working_tree_side(
                    root_path,
                    &entry.path,
                    &entry.worktree_mode,
                    entry.untracked,
                )
                .await?
            })
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

/// Reject a `mode` that is not one of git's three ordinary file modes
/// before it ever reaches `git update-index --cacheinfo`. `--cacheinfo`
/// writes its `mode,blob,path` triple into the index verbatim with no
/// validation of its own, so an unchecked mode string (for example
/// `100644,evil` smuggling extra `--cacheinfo` fields, or `160000`
/// fabricating a submodule entry) would be staged exactly as given.
fn validate_mode(mode: &str) -> Result<(), ReviewError> {
    match mode {
        "100644" | "100755" | "120000" => Ok(()),
        other => {
            tracing::warn!(mode = %other, "rejecting unrecognized staging mode");
            Err(ReviewError::NotStageable(format!(
                "{other} is not a stageable file mode"
            )))
        }
    }
}

/// Reject a `repository_root` argument that is not exactly the toplevel
/// path `git rev-parse --show-toplevel` resolves for it (a trailing slash,
/// a subdirectory, or any other variant). The frontend always echoes back
/// the exact `repository_root` string `changes()` returned, which is
/// already the toplevel, so any mismatch means an untrusted or stale
/// caller — and the allowlist keyed by that string in the application
/// layer's `ReviewService` only holds if the key is canonical.
async fn verify_repository_root(given_root: &str) -> Result<(), ReviewError> {
    let resolved = resolve_repository_root(given_root).await?;
    if resolved != given_root {
        tracing::warn!(
            given = %given_root,
            resolved = %resolved,
            "repository root is not its own git toplevel; rejecting"
        );
        return Err(ReviewError::NotStageable(format!(
            "{given_root} is not a repository toplevel"
        )));
    }
    Ok(())
}

/// Refuse to stage or unstage a path that is absolute or escapes the
/// repository via a `..` component. This is defense in depth underneath the
/// application-layer safety boundary (a path must also appear in the most
/// recently loaded stageable change set) — the git layer must independently
/// never hand an attacker-controlled path straight to `git update-index` or
/// `git restore`.
fn validate_relative_path(path: &str) -> Result<(), ReviewError> {
    if path.is_empty() {
        return Err(ReviewError::NotStageable(
            "path must not be empty".to_string(),
        ));
    }
    if path.contains('\0') || path.contains('\n') {
        tracing::warn!("rejecting a stageable path containing a control character");
        return Err(ReviewError::NotStageable(
            "path contains a disallowed control character".to_string(),
        ));
    }

    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(ReviewError::NotStageable(format!(
            "{path}: absolute paths are not stageable"
        )));
    }
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(ReviewError::NotStageable(format!(
            "{path}: paths containing `..` are not stageable"
        )));
    }

    Ok(())
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
    // A gitlink's "blob" id actually names a commit object (M1:
    // submodules), so it is excluded here and never fetched at all.
    let mut blob_ids: Vec<String> = Vec::new();
    for entry in &entries {
        if entry.src_mode != ZERO_MODE && !is_gitlink_mode(&entry.src_mode) {
            blob_ids.push(entry.src_blob.clone());
        }
        if !target_from_disk && entry.dst_mode != ZERO_MODE && !is_gitlink_mode(&entry.dst_mode) {
            blob_ids.push(entry.dst_blob.clone());
        }
    }
    let blobs = batch_read_blobs(root_path, &blob_ids).await?;

    let mut files = Vec::with_capacity(entries.len());
    for entry in &entries {
        let base = if entry.src_mode != ZERO_MODE {
            Some(if is_gitlink_mode(&entry.src_mode) {
                gitlink_side(&entry.path, &entry.src_blob)
            } else {
                file_side_from_blob(&blobs, &entry.src_blob, &entry.src_mode)?
            })
        } else {
            None
        };

        let target = if entry.untracked {
            Some(read_working_tree_side(root_path, &entry.path, "", true).await?)
        } else if target_from_disk {
            if entry.dst_mode == ZERO_MODE {
                None
            } else if is_gitlink_mode(&entry.dst_mode) {
                Some(gitlink_side(&entry.path, &entry.dst_blob))
            } else {
                Some(read_working_tree_side(root_path, &entry.path, &entry.dst_mode, false).await?)
            }
        } else if entry.dst_mode != ZERO_MODE {
            Some(if is_gitlink_mode(&entry.dst_mode) {
                gitlink_side(&entry.path, &entry.dst_blob)
            } else {
                file_side_from_blob(&blobs, &entry.dst_blob, &entry.dst_mode)?
            })
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

/// Mode `160000` is a submodule gitlink (M1): its "blob" id names a COMMIT
/// in the submodule's own history, not a blob in this repository, and must
/// never be read as file content.
fn is_gitlink_mode(mode: &str) -> bool {
    mode == "160000"
}

/// The commit id to show for a gitlink placeholder: prefer the index's
/// pinned commit (the one that would actually be staged), falling back to
/// HEAD's when the index has none (for example a staged deletion).
fn gitlink_reference(entry: &StatusEntry) -> &str {
    if !entry.index_blob.is_empty() {
        &entry.index_blob
    } else {
        &entry.head_blob
    }
}

/// Build a placeholder `FileSide` for a submodule gitlink (M1): its blob id
/// names a commit, not a blob, so no content is ever attached — only
/// `binary: true`, the same signal the viewer already uses for any file it
/// will not render as text.
fn gitlink_side(path: &str, commit_id: &str) -> FileSide {
    tracing::debug!(
        path = %path,
        commit = %commit_id,
        "submodule gitlink encountered; rendering a placeholder instead of commit content"
    );
    FileSide {
        content: None,
        blob: commit_id.to_string(),
        mode: "160000".to_string(),
        binary: true,
        too_large: false,
    }
}

fn language_for_path(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(language_for_extension)
}

/// Build a `FileSide` for a blob side (base or index) from an entry already
/// resolved by the two-pass batched `cat-file --batch-check`/`--batch` call
/// (I2): an oversized blob's `content` is `None` (its bytes were never
/// fetched at all, not fetched and discarded) and reports `too_large`.
fn file_side_from_blob(
    blobs: &HashMap<String, BlobEntry>,
    blob_id: &str,
    mode: &str,
) -> Result<FileSide, ReviewError> {
    let entry = blobs.get(blob_id).ok_or_else(|| {
        ReviewError::GitFailed(format!("blob {blob_id} missing from cat-file batch"))
    })?;

    let mut binary = false;
    let mut content = None;
    match &entry.content {
        Some(bytes) => {
            if is_likely_binary(bytes) {
                binary = true;
            } else {
                match std::str::from_utf8(bytes) {
                    Ok(text) => content = Some(text.to_string()),
                    Err(_) => binary = true,
                }
            }
        }
        None if !entry.too_large => {
            // Content was deliberately not fetched for a reason other than
            // size (for example an unexpected non-blob object slipping
            // through); never claim it is renderable text.
            binary = true;
        }
        None => {}
    }

    Ok(FileSide {
        content,
        blob: blob_id.to_string(),
        mode: mode.to_string(),
        binary,
        too_large: entry.too_large,
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
///
/// **Never dereferences a symlink (C1).** `symlink_metadata` (not
/// `metadata`) is used to stat the path, so a symlinked path is identified
/// as a symlink rather than followed to whatever it points at — which may
/// sit outside the repository entirely. A symlink's side is built from
/// `read_link`'s raw bytes (the link target string, exactly as git itself
/// stores a symlink blob), hashed by piping those bytes to
/// `git hash-object -w --stdin --no-filters`; the file the link points at
/// is never opened.
async fn read_working_tree_side(
    root_path: &Path,
    path: &str,
    worktree_mode: &str,
    untracked: bool,
) -> Result<FileSide, ReviewError> {
    let file_path = root_path.join(path);

    let metadata = tokio::fs::symlink_metadata(&file_path)
        .await
        .map_err(|error| {
            tracing::warn!(path = %file_path.display(), %error, "failed to stat working tree file");
            ReviewError::Io(error.to_string())
        })?;

    if metadata.file_type().is_symlink() || worktree_mode == "120000" {
        return read_working_tree_symlink(root_path, &file_path).await;
    }

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

/// Build the working-tree `FileSide` for a symlink: content is the raw
/// bytes `read_link` returns (the link target string), mode is always
/// `120000` whether the symlink is tracked or untracked, and the blob id
/// comes from feeding those exact bytes to
/// `git hash-object -w --stdin --no-filters` over stdin — the symlink's
/// target path is never opened, so bytes from outside the repository (or
/// from a file the symlink targets) can never leak into the diff or the
/// object database.
async fn read_working_tree_symlink(
    root_path: &Path,
    file_path: &Path,
) -> Result<FileSide, ReviewError> {
    let link_target = tokio::fs::read_link(file_path).await.map_err(|error| {
        tracing::warn!(path = %file_path.display(), %error, "failed to read symlink target");
        ReviewError::Io(error.to_string())
    })?;
    let target_bytes = link_target.as_os_str().as_bytes().to_vec();

    let hash_output = run_git(
        root_path,
        &["hash-object", "-w", "--stdin", "--no-filters"],
        Some(&target_bytes),
    )
    .await?;
    let blob = String::from_utf8_lossy(&hash_output).trim().to_string();

    let content = String::from_utf8(target_bytes).ok();
    let binary = content.is_none();

    Ok(FileSide {
        content,
        blob,
        mode: "120000".to_string(),
        binary,
        too_large: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_mode_accepts_the_three_valid_modes() {
        for mode in ["100644", "100755", "120000"] {
            assert!(
                validate_mode(mode).is_ok(),
                "{mode} must be accepted as a valid staging mode"
            );
        }
    }

    #[test]
    fn validate_mode_rejects_a_mode_with_trailing_garbage() {
        let error = validate_mode("100644,evil").expect_err("must be rejected");
        assert!(matches!(error, ReviewError::NotStageable(_)));
    }

    #[test]
    fn validate_mode_rejects_a_submodule_mode() {
        let error = validate_mode("160000").expect_err("must be rejected");
        assert!(matches!(error, ReviewError::NotStageable(_)));
    }
}
