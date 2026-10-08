//! Batches blob content lookups into one `git cat-file --batch` process.

use std::collections::HashMap;
use std::path::Path;

use quantum_domain::ReviewError;

use crate::runner::run_git;

/// Fetch the raw bytes of every blob id in `blob_ids` using a single
/// `git cat-file --batch` process (stdin: one blob id per line), rather than
/// spawning a process per blob. Duplicate ids are deduplicated before the
/// call and the returned map has one entry per unique id.
///
/// A missing object (reported by `cat-file --batch` as `<id> missing`) maps
/// the whole call to [`ReviewError::GitFailed`]: every blob id this crate
/// asks for was reported by `git status`/`git diff` moments earlier, so a
/// missing object means the repository changed under us or git lied, either
/// of which is worth surfacing rather than silently dropping that file.
pub(crate) async fn batch_read_blobs(
    root: &Path,
    blob_ids: &[String],
) -> Result<HashMap<String, Vec<u8>>, ReviewError> {
    if blob_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut unique: Vec<&str> = Vec::new();
    for id in blob_ids {
        if !unique.contains(&id.as_str()) {
            unique.push(id.as_str());
        }
    }

    let mut stdin = String::new();
    for id in &unique {
        stdin.push_str(id);
        stdin.push('\n');
    }

    let output = run_git(root, &["cat-file", "--batch"], Some(stdin.as_bytes())).await?;
    parse_batch_output(&output, unique.len())
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
