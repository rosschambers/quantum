//! Parses the output of `git status --porcelain=v2 -z --untracked-files=all`.

/// One entry from `git status --porcelain=v2`. Mirrors the two left-hand
/// status columns (`X` index status, `Y` worktree status) alongside the
/// blob ids and file modes porcelain v2 reports for ordinary and
/// renamed/copied entries. The submodule state field is parsed but
/// discarded: this project treats submodules as plain tracked content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusEntry {
    pub path: String,
    pub old_path: Option<String>,
    pub index_status: char,
    pub worktree_status: char,
    pub head_mode: String,
    pub index_mode: String,
    pub worktree_mode: String,
    pub head_blob: String,
    pub index_blob: String,
    pub untracked: bool,
}

/// Parse NUL-separated `git status --porcelain=v2 -z --untracked-files=all`
/// output into entries.
///
/// Unmerged (`u`) records are skipped with a warning (this project has no
/// conflict-resolution UI); ignored (`!`) records are skipped silently (not
/// relevant to a diff view). A rename/copy (`2`) record's second path
/// (`origPath`) rides in the NEXT NUL-delimited chunk with no header of its
/// own, so the main loop advances past one extra chunk after consuming it.
pub(crate) fn parse_status(output: &[u8]) -> Vec<StatusEntry> {
    let chunks: Vec<&[u8]> = output
        .split(|&byte| byte == 0)
        .filter(|chunk| !chunk.is_empty())
        .collect();

    let mut entries = Vec::new();
    let mut index = 0;
    while index < chunks.len() {
        let text = String::from_utf8_lossy(chunks[index]);

        if let Some(rest) = text.strip_prefix("1 ") {
            if let Some(entry) = parse_ordinary(rest) {
                entries.push(entry);
            } else {
                tracing::warn!(record = %text, "malformed ordinary git status record, skipping");
            }
        } else if let Some(rest) = text.strip_prefix("2 ") {
            index += 1;
            let old_path = chunks
                .get(index)
                .map(|chunk| String::from_utf8_lossy(chunk).into_owned());
            match parse_rename(rest) {
                Some(mut entry) => {
                    entry.old_path = old_path;
                    entries.push(entry);
                }
                None => {
                    tracing::warn!(record = %text, "malformed rename git status record, skipping");
                }
            }
        } else if text.starts_with("u ") {
            tracing::warn!(
                record = %text,
                "skipping unmerged git status entry: no conflict resolution UI yet"
            );
        } else if text.starts_with('!') {
            // Ignored: never relevant to a diff view, skip silently.
        } else if let Some(path) = text.strip_prefix("? ") {
            entries.push(StatusEntry {
                path: path.to_string(),
                old_path: None,
                index_status: '?',
                worktree_status: '?',
                head_mode: String::new(),
                index_mode: String::new(),
                worktree_mode: String::new(),
                head_blob: String::new(),
                index_blob: String::new(),
                untracked: true,
            });
        } else {
            tracing::warn!(record = %text, "unrecognized git status record, skipping");
        }

        index += 1;
    }

    entries
}

/// Parse the tail of an ordinary (`1`) record: `XY sub mH mI mW hH hI path`.
fn parse_ordinary(rest: &str) -> Option<StatusEntry> {
    let mut fields = rest.splitn(8, ' ');
    let xy = fields.next()?;
    let _sub = fields.next()?;
    let head_mode = fields.next()?.to_string();
    let index_mode = fields.next()?.to_string();
    let worktree_mode = fields.next()?.to_string();
    let head_blob = fields.next()?.to_string();
    let index_blob = fields.next()?.to_string();
    let path = fields.next()?.to_string();
    let mut xy_chars = xy.chars();
    let index_status = xy_chars.next()?;
    let worktree_status = xy_chars.next()?;

    Some(StatusEntry {
        path,
        old_path: None,
        index_status,
        worktree_status,
        head_mode,
        index_mode,
        worktree_mode,
        head_blob,
        index_blob,
        untracked: false,
    })
}

/// Parse the tail of a rename/copy (`2`) record:
/// `XY sub mH mI mW hH hI Xscore path`. The caller fills `old_path` from the
/// following chunk.
fn parse_rename(rest: &str) -> Option<StatusEntry> {
    let mut fields = rest.splitn(9, ' ');
    let xy = fields.next()?;
    let _sub = fields.next()?;
    let head_mode = fields.next()?.to_string();
    let index_mode = fields.next()?.to_string();
    let worktree_mode = fields.next()?.to_string();
    let head_blob = fields.next()?.to_string();
    let index_blob = fields.next()?.to_string();
    let _score = fields.next()?;
    let path = fields.next()?.to_string();
    let mut xy_chars = xy.chars();
    let index_status = xy_chars.next()?;
    let worktree_status = xy_chars.next()?;

    Some(StatusEntry {
        path,
        old_path: None,
        index_status,
        worktree_status,
        head_mode,
        index_mode,
        worktree_mode,
        head_blob,
        index_blob,
        untracked: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOB_A: &str = "1111111111111111111111111111111111111111";
    const BLOB_B: &str = "2222222222222222222222222222222222222222";
    const BLOB_C: &str = "3333333333333333333333333333333333333333";
    const ZERO: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn modified_unstaged() {
        let record =
            format!("1 .M N... 100644 100644 100644 {BLOB_A} {BLOB_A} file.txt\0").into_bytes();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.path, "file.txt");
        assert_eq!(entry.index_status, '.');
        assert_eq!(entry.worktree_status, 'M');
        assert_eq!(entry.head_mode, "100644");
        assert_eq!(entry.index_mode, "100644");
        assert_eq!(entry.worktree_mode, "100644");
        assert_eq!(entry.head_blob, BLOB_A);
        assert_eq!(entry.index_blob, BLOB_A);
        assert!(!entry.untracked);
        assert_eq!(entry.old_path, None);
    }

    #[test]
    fn staged_modification() {
        let record =
            format!("1 M. N... 100644 100644 100644 {BLOB_A} {BLOB_B} file.txt\0").into_bytes();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.index_status, 'M');
        assert_eq!(entry.worktree_status, '.');
        assert_eq!(entry.head_blob, BLOB_A);
        assert_eq!(entry.index_blob, BLOB_B);
    }

    #[test]
    fn partially_staged() {
        let record =
            format!("1 MM N... 100644 100644 100644 {BLOB_A} {BLOB_B} file.txt\0").into_bytes();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.index_status, 'M');
        assert_eq!(entry.worktree_status, 'M');
        assert_eq!(entry.head_blob, BLOB_A);
        assert_eq!(entry.index_blob, BLOB_B);
    }

    #[test]
    fn added() {
        let record =
            format!("1 A. N... 000000 100644 100644 {ZERO} {BLOB_C} newfile.txt\0").into_bytes();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.index_status, 'A');
        assert_eq!(entry.worktree_status, '.');
        assert_eq!(entry.head_mode, "000000");
        assert_eq!(entry.head_blob, ZERO);
        assert_eq!(entry.index_blob, BLOB_C);
    }

    #[test]
    fn deleted_unstaged() {
        let record =
            format!("1 .D N... 100644 100644 000000 {BLOB_A} {BLOB_A} oldfile.txt\0").into_bytes();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.index_status, '.');
        assert_eq!(entry.worktree_status, 'D');
        assert_eq!(entry.worktree_mode, "000000");
    }

    #[test]
    fn renamed_staged_with_orig_path() {
        let record = format!(
            "2 R. N... 100644 100644 100644 {BLOB_A} {BLOB_A} R100 newname.txt\0oldname.txt\0"
        )
        .into_bytes();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.path, "newname.txt");
        assert_eq!(entry.old_path, Some("oldname.txt".to_string()));
        assert_eq!(entry.index_status, 'R');
        assert_eq!(entry.worktree_status, '.');
    }

    #[test]
    fn untracked() {
        let record = b"? untracked.txt\0".to_vec();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.path, "untracked.txt");
        assert!(entry.untracked);
        assert_eq!(entry.head_mode, "");
        assert_eq!(entry.head_blob, "");
    }

    #[test]
    fn path_with_spaces_survives() {
        let record =
            format!("1 .M N... 100644 100644 100644 {BLOB_A} {BLOB_A} path with spaces.txt\0")
                .into_bytes();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "path with spaces.txt");
    }

    #[test]
    fn unmerged_is_skipped() {
        let record = format!(
            "u UU N... 100644 100644 100644 100644 {BLOB_A} {BLOB_B} {BLOB_C} conflict.txt\0"
        )
        .into_bytes();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn ignored_is_skipped_silently() {
        let record = b"! ignored.txt\0".to_vec();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn multiple_records_parse_in_order() {
        let record =
            format!("1 .M N... 100644 100644 100644 {BLOB_A} {BLOB_A} first.txt\0? second.txt\0")
                .into_bytes();
        let entries = parse_status(&record);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "first.txt");
        assert_eq!(entries[1].path, "second.txt");
        assert!(entries[1].untracked);
    }
}
