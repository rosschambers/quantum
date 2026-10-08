//! Batches blob content lookups into at most two `git cat-file` processes:
//! one `--batch-check` pass to learn each object's type and size, then one
//! `--batch` pass fetching only the blobs small enough to show (I2) — an
//! oversized committed blob is never read into memory at all, and a
//! non-blob object (for example a submodule's commit, guarded against
//! separately by mode in `review.rs`) is never fetched either.

use std::collections::HashMap;
use std::path::Path;

use quantum_domain::{ReviewError, VIEWER_TEXT_MAX_BYTES};

use crate::runner::run_git;

/// The resolved content for one requested blob id: `content` is `None` when
/// the object was skipped — either because it was over the size cap
/// (`too_large`) or because it was not a blob at all (missing or a non-blob
/// object type), never because its bytes were fetched and then discarded.
pub(crate) struct BlobEntry {
    pub content: Option<Vec<u8>>,
    pub too_large: bool,
}

/// Fetch the content of every blob id in `blob_ids`, skipping anything over
/// [`VIEWER_TEXT_MAX_BYTES`] before it is ever read into memory. Duplicate
/// ids are deduplicated before either call and the returned map has one
/// entry per unique id.
///
/// A missing object (reported by `cat-file --batch-check` as `<id>
/// missing`) maps the whole call to [`ReviewError::GitFailed`]: every blob
/// id this crate asks for was reported by `git status`/`git diff` moments
/// earlier, so a missing object means the repository changed under us or
/// git lied, either of which is worth surfacing rather than silently
/// dropping that file.
pub(crate) async fn batch_read_blobs(
    root: &Path,
    blob_ids: &[String],
) -> Result<HashMap<String, BlobEntry>, ReviewError> {
    if blob_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut unique: Vec<&str> = Vec::new();
    for id in blob_ids {
        if !unique.contains(&id.as_str()) {
            unique.push(id.as_str());
        }
    }

    let check_output = run_git(
        root,
        &["cat-file", "--batch-check"],
        Some(lines_stdin(&unique).as_bytes()),
    )
    .await?;
    let records = parse_batch_check(&check_output);
    if records.len() != unique.len() {
        let message = "git cat-file --batch-check: truncated output".to_string();
        tracing::warn!(%message);
        return Err(ReviewError::GitFailed(message));
    }
    for record in &records {
        if record.object_type.is_none() {
            tracing::warn!(blob = %record.id, "git cat-file --batch-check reported a missing object");
            return Err(ReviewError::GitFailed(format!(
                "object not found: {}",
                record.id
            )));
        }
    }

    let (to_fetch, mut resolved) = partition_batch_check(&records, VIEWER_TEXT_MAX_BYTES);

    if !to_fetch.is_empty() {
        let fetch_ids: Vec<&str> = to_fetch.iter().map(String::as_str).collect();
        let fetch_output = run_git(
            root,
            &["cat-file", "--batch"],
            Some(lines_stdin(&fetch_ids).as_bytes()),
        )
        .await?;
        let fetched = parse_batch_output(&fetch_output, to_fetch.len())?;
        for (id, bytes) in fetched {
            resolved.insert(
                id,
                BlobEntry {
                    content: Some(bytes),
                    too_large: false,
                },
            );
        }
    }

    Ok(resolved)
}

fn lines_stdin(ids: &[&str]) -> String {
    let mut stdin = String::new();
    for id in ids {
        stdin.push_str(id);
        stdin.push('\n');
    }
    stdin
}

/// One record from `git cat-file --batch-check` output: either
/// `<id> <type> <size>` (`object_type`/`size` both `Some`) or `<id> missing`
/// (`object_type`/`size` both `None`).
pub(crate) struct BatchCheckRecord {
    pub id: String,
    pub object_type: Option<String>,
    pub size: Option<u64>,
}

/// Parse `git cat-file --batch-check` output: one line per requested id,
/// `<id> <type> <size>\n` or `<id> missing\n`.
fn parse_batch_check(output: &[u8]) -> Vec<BatchCheckRecord> {
    String::from_utf8_lossy(output)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut fields = line.split(' ');
            let id = fields.next().unwrap_or_default().to_string();
            match (fields.next(), fields.next()) {
                (Some(object_type), Some(size_field)) => BatchCheckRecord {
                    id,
                    object_type: Some(object_type.to_string()),
                    size: size_field.trim().parse().ok(),
                },
                _ => BatchCheckRecord {
                    id,
                    object_type: None,
                    size: None,
                },
            }
        })
        .collect()
}

/// Split parsed `--batch-check` records into the ids worth fetching full
/// content for (blobs at or under `max_bytes`) and a map of everything
/// already resolved without fetching: oversized blobs (`too_large: true`,
/// no content) and non-blob objects (no content, not flagged `too_large` —
/// callers should normally avoid requesting these at all; see the
/// mode-160000 guard in `review.rs`). This is a pure function so the
/// exclusion of oversized ids from the fetch list is testable without
/// spawning git at all.
pub(crate) fn partition_batch_check(
    records: &[BatchCheckRecord],
    max_bytes: u64,
) -> (Vec<String>, HashMap<String, BlobEntry>) {
    let mut to_fetch = Vec::new();
    let mut resolved = HashMap::new();

    for record in records {
        let Some(object_type) = &record.object_type else {
            // Missing objects are handled (and turned into an error) by the
            // caller before this function runs; treat defensively here too.
            resolved.insert(
                record.id.clone(),
                BlobEntry {
                    content: None,
                    too_large: false,
                },
            );
            continue;
        };

        let size = record.size.unwrap_or(0);
        if object_type == "blob" && size <= max_bytes {
            to_fetch.push(record.id.clone());
        } else if object_type == "blob" {
            resolved.insert(
                record.id.clone(),
                BlobEntry {
                    content: None,
                    too_large: true,
                },
            );
        } else {
            resolved.insert(
                record.id.clone(),
                BlobEntry {
                    content: None,
                    too_large: false,
                },
            );
        }
    }

    (to_fetch, resolved)
}

/// Parse `git cat-file --batch` output: `expected` records of either
/// `<id> <type> <size>\n<content bytes>\n` or `<id> missing\n`.
fn parse_batch_output(
    output: &[u8],
    expected: usize,
) -> Result<HashMap<String, Vec<u8>>, ReviewError> {
    let mut results = HashMap::with_capacity(expected);
    let mut cursor = 0;

    for _ in 0..expected {
        let header_end = find_newline(output, cursor).ok_or_else(|| {
            let message = "git cat-file --batch: truncated output (missing header)".to_string();
            tracing::warn!(%message);
            ReviewError::GitFailed(message)
        })?;
        let header = String::from_utf8_lossy(&output[cursor..header_end]).into_owned();
        cursor = header_end + 1;

        let mut fields = header.split(' ');
        let id = fields.next().unwrap_or_default().to_string();

        if header.ends_with("missing") {
            tracing::warn!(blob = %id, "git cat-file --batch reported a missing object");
            return Err(ReviewError::GitFailed(format!("object not found: {id}")));
        }

        let _object_type = fields.next().ok_or_else(|| {
            ReviewError::GitFailed(format!("git cat-file --batch: malformed header: {header}"))
        })?;
        let size_field = fields.next().ok_or_else(|| {
            ReviewError::GitFailed(format!("git cat-file --batch: malformed header: {header}"))
        })?;
        let size: usize = size_field.trim().parse().map_err(|_| {
            ReviewError::GitFailed(format!("git cat-file --batch: invalid size in: {header}"))
        })?;

        let content_end = cursor + size;
        if content_end > output.len() {
            let message = "git cat-file --batch: truncated output (short content)".to_string();
            tracing::warn!(%message);
            return Err(ReviewError::GitFailed(message));
        }
        let bytes = output[cursor..content_end].to_vec();
        cursor = content_end;
        // Each content block is followed by a trailing newline before the
        // next record's header.
        if output.get(cursor) == Some(&b'\n') {
            cursor += 1;
        }

        results.insert(id, bytes);
    }

    Ok(results)
}

fn find_newline(buffer: &[u8], from: usize) -> Option<usize> {
    buffer[from..]
        .iter()
        .position(|&byte| byte == b'\n')
        .map(|position| from + position)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL_BLOB: &str = "1111111111111111111111111111111111111111";
    const LARGE_BLOB: &str = "2222222222222222222222222222222222222222";
    const COMMIT_OBJECT: &str = "3333333333333333333333333333333333333333";
    const MISSING_BLOB: &str = "4444444444444444444444444444444444444444";

    #[test]
    fn parse_batch_check_reads_type_and_size() {
        let output = format!("{SMALL_BLOB} blob 42\n{MISSING_BLOB} missing\n").into_bytes();
        let records = parse_batch_check(&output);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, SMALL_BLOB);
        assert_eq!(records[0].object_type.as_deref(), Some("blob"));
        assert_eq!(records[0].size, Some(42));
        assert_eq!(records[1].id, MISSING_BLOB);
        assert_eq!(records[1].object_type, None);
        assert_eq!(records[1].size, None);
    }

    #[test]
    fn oversized_blob_is_excluded_from_the_fetch_list_and_flagged_too_large() {
        let records = vec![
            BatchCheckRecord {
                id: SMALL_BLOB.to_string(),
                object_type: Some("blob".to_string()),
                size: Some(10),
            },
            BatchCheckRecord {
                id: LARGE_BLOB.to_string(),
                object_type: Some("blob".to_string()),
                size: Some(1_000),
            },
        ];

        let (to_fetch, resolved) = partition_batch_check(&records, 100);

        assert_eq!(to_fetch, vec![SMALL_BLOB.to_string()]);
        assert!(
            !to_fetch.contains(&LARGE_BLOB.to_string()),
            "a blob over the size cap must never be requested from `cat-file --batch`"
        );
        let large_entry = resolved.get(LARGE_BLOB).expect("large blob resolved");
        assert!(large_entry.too_large);
        assert!(large_entry.content.is_none());
    }

    #[test]
    fn a_non_blob_object_is_never_fetched_or_treated_as_content() {
        // Defensive: review.rs already skips mode-160000 (gitlink) entries
        // before adding them to the batch at all, but this proves the
        // batching layer independently never sends a non-blob object's
        // bytes as file content if one slips through.
        let records = vec![BatchCheckRecord {
            id: COMMIT_OBJECT.to_string(),
            object_type: Some("commit".to_string()),
            size: Some(200),
        }];

        let (to_fetch, resolved) = partition_batch_check(&records, 1_000_000);

        assert!(to_fetch.is_empty());
        let entry = resolved.get(COMMIT_OBJECT).expect("commit object resolved");
        assert!(entry.content.is_none());
    }

    #[test]
    fn exactly_the_size_cap_is_still_fetched() {
        let records = vec![BatchCheckRecord {
            id: SMALL_BLOB.to_string(),
            object_type: Some("blob".to_string()),
            size: Some(100),
        }];

        let (to_fetch, resolved) = partition_batch_check(&records, 100);

        assert_eq!(to_fetch, vec![SMALL_BLOB.to_string()]);
        assert!(resolved.is_empty());
    }
}
