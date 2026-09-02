//! Clipboard-history provider.
//!
//! Provider id `clipboard`. Responds only to the `;` prefix: the launcher shows
//! recent clipboard entries, newest first, filtered by the text after `;`.
//! Selecting an entry re-copies it to the system clipboard; menu actions delete
//! a single entry or clear the whole history.

use std::sync::Arc;

use async_trait::async_trait;
use quantum_domain::{
    Action, ActionOutcome, ClipboardEntry, ClipboardError, ClipboardStore, ClipboardWriter,
    DomainError, IconRef, Match, MatchScore, MenuAction, ProviderId, ProviderSource, Query,
};

/// Provider surfacing clipboard history behind the `;` prefix.
pub struct ClipboardProvider {
    id: ProviderId,
    store: Arc<dyn ClipboardStore>,
    clipboard: Arc<dyn ClipboardWriter>,
}

impl ClipboardProvider {
    /// Create a provider over the shared clipboard `store` and `clipboard`
    /// writer.
    pub fn new(store: Arc<dyn ClipboardStore>, clipboard: Arc<dyn ClipboardWriter>) -> Self {
        Self {
            id: ProviderId::from("clipboard"),
            store,
            clipboard,
        }
    }
}

/// Map a store error onto the domain error surfaced across IPC.
fn map_store_error(error: ClipboardError) -> DomainError {
    match error {
        ClipboardError::NotFound(id) => DomainError::NotFound(id),
        ClipboardError::Persistence(reason) => DomainError::ActionFailed { reason },
    }
}

/// Current time in whole seconds since the Unix epoch, for relative-age
/// display. Falls back to zero if the clock is before the epoch.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// A short human-readable age like `just now`, `5m ago`, `3h ago`, `2d ago`.
fn relative_age(created_unix: u64, now_unix: u64) -> String {
    let seconds = now_unix.saturating_sub(created_unix);
    if seconds < 60 {
        "just now".to_string()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}

/// Collapse whitespace to single spaces, trim, and cap the length for a
/// single-line title.
fn single_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 80;
    if collapsed.chars().count() > MAX {
        collapsed.chars().take(MAX).collect::<String>() + "…"
    } else {
        collapsed
    }
}

/// The trailing path component of a blob path, or the whole string when it has
/// no separator, used as a binary entry's display name fallback.
fn file_name_or(mime: &str, blob_path: &str) -> String {
    let name = blob_path.rsplit('/').next().unwrap_or(blob_path);
    if name.is_empty() {
        mime.to_string()
    } else {
        name.to_string()
    }
}

/// True when a text entry matches the lowercased `needle` in its preview or
/// full text.
fn text_matches(preview: &str, full: &str, needle: &str) -> bool {
    preview.to_lowercase().contains(needle) || full.to_lowercase().contains(needle)
}

/// Build the `Custom` action envelope for a clipboard operation on `id`.
fn custom_op(op: &str, id: Option<&str>) -> Action {
    let payload = match id {
        Some(id) => serde_json::json!({ "op": op, "id": id }),
        None => serde_json::json!({ "op": op }),
    };
    Action::Custom {
        kind: "clipboard".to_string(),
        payload,
    }
}

impl ClipboardProvider {
    /// The three per-entry menu actions common to every kind: recopy, delete,
    /// clear.
    fn entry_actions(&self, id: &str, primary: Action) -> Vec<MenuAction> {
        vec![
            MenuAction {
                label: "Copy".to_string(),
                icon: None,
                danger: false,
                action: primary,
            },
            MenuAction {
                label: "Delete entry".to_string(),
                icon: None,
                danger: true,
                action: custom_op("delete", Some(id)),
            },
            MenuAction {
                label: "Clear history".to_string(),
                icon: None,
                danger: true,
                action: custom_op("clear", None),
            },
        ]
    }

    /// Build the [`Match`] for one clipboard entry, given the current time for
    /// relative-age display.
    fn build_match(&self, entry: &ClipboardEntry, now: u64) -> Match {
        let age = relative_age(entry.created_unix(), now);
        match entry {
            ClipboardEntry::Text {
                id, preview, full, ..
            } => {
                let primary = Action::Copy { text: full.clone() };
                Match {
                    id: id.clone(),
                    provider: self.id.clone(),
                    title: single_line(preview),
                    subtitle: Some(age),
                    icon: None,
                    score: MatchScore::new(1.0),
                    action: primary.clone(),
                    actions: self.entry_actions(id, primary),
                }
            }
            ClipboardEntry::Image {
                id,
                preview_thumb,
                width,
                height,
                ..
            } => {
                let primary = custom_op("recopy", Some(id));
                Match {
                    id: id.clone(),
                    provider: self.id.clone(),
                    title: format!("Image {width}x{height}"),
                    subtitle: Some(age),
                    icon: Some(IconRef::DataUri(preview_thumb.clone())),
                    score: MatchScore::new(1.0),
                    action: primary.clone(),
                    actions: self.entry_actions(id, primary),
                }
            }
            ClipboardEntry::Binary {
                id,
                mime,
                blob_path,
                ..
            } => {
                let primary = custom_op("recopy", Some(id));
                Match {
                    id: id.clone(),
                    provider: self.id.clone(),
                    title: file_name_or(mime, blob_path),
                    subtitle: Some(age),
                    icon: Some(IconRef::Name("text-x-generic".to_string())),
                    score: MatchScore::new(1.0),
                    action: primary.clone(),
                    actions: self.entry_actions(id, primary),
                }
            }
        }
    }

    /// Re-offer the entry with `id` to the system clipboard: text is written
    /// directly; blob-backed kinds have their bytes read and written under the
    /// appropriate MIME type.
    async fn recopy(&self, id: &str) -> Result<ActionOutcome, DomainError> {
        let data = self.store.load().await.map_err(map_store_error)?;
        let entry = data
            .entries
            .into_iter()
            .find(|entry| entry.id() == id)
            .ok_or_else(|| DomainError::NotFound(id.to_string()))?;

        match entry {
            ClipboardEntry::Text { full, .. } => {
                self.clipboard.write_text(&full).await?;
            }
            ClipboardEntry::Image { blob_path, .. } => {
                let bytes = read_blob(&blob_path)?;
                self.clipboard.write_bytes("image/png", &bytes).await?;
            }
            ClipboardEntry::Binary {
                blob_path, mime, ..
            } => {
                let bytes = read_blob(&blob_path)?;
                self.clipboard.write_bytes(&mime, &bytes).await?;
            }
        }
        Ok(ActionOutcome {
            message: Some("Copied".to_string()),
        })
    }

    /// Handle a `Custom { kind: "clipboard" }` action by dispatching on its
    /// `payload.op` field.
    async fn invoke_custom(
        &self,
        payload: &serde_json::Value,
    ) -> Result<ActionOutcome, DomainError> {
        let op = payload.get("op").and_then(|value| value.as_str());
        match op {
            Some("recopy") => {
                let id = payload.get("id").and_then(|value| value.as_str());
                match id {
                    Some(id) => self.recopy(id).await,
                    None => Err(DomainError::InvalidQuery(
                        "clipboard recopy requires an id".to_string(),
                    )),
                }
            }
            Some("delete") => {
                let id = payload.get("id").and_then(|value| value.as_str());
                match id {
                    Some(id) => {
                        self.store.remove(id).await.map_err(map_store_error)?;
                        Ok(ActionOutcome {
                            message: Some("Deleted".to_string()),
                        })
                    }
                    None => Err(DomainError::InvalidQuery(
                        "clipboard delete requires an id".to_string(),
                    )),
                }
            }
            Some("clear") => {
                self.store.clear().await.map_err(map_store_error)?;
                Ok(ActionOutcome {
                    message: Some("Cleared".to_string()),
                })
            }
            other => Err(DomainError::Unsupported(format!(
                "unknown clipboard operation: {}",
                other.unwrap_or("<missing>")
            ))),
        }
    }
}

/// Read a blob file, mapping a read failure to a domain error.
fn read_blob(path: &str) -> Result<Vec<u8>, DomainError> {
    std::fs::read(path).map_err(|error| {
        tracing::warn!(path, %error, "failed to read clipboard blob for recopy");
        DomainError::ActionFailed {
            reason: format!("read clipboard blob {path}: {error}"),
        }
    })
}

#[async_trait]
impl ProviderSource for ClipboardProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError> {
        let needle = match q.text.strip_prefix(';') {
            Some(rest) => rest.trim().to_lowercase(),
            None => return Ok(vec![]),
        };

        let data = self.store.load().await.map_err(map_store_error)?;
        let now = now_unix();

        // Newest first.
        let mut entries = data.entries;
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_unix()));

        let matches = entries
            .iter()
            .filter(|entry| {
                if needle.is_empty() {
                    return true;
                }
                match entry {
                    ClipboardEntry::Text { preview, full, .. } => {
                        text_matches(preview, full, &needle)
                    }
                    // Non-text entries are only shown for the empty query.
                    _ => false,
                }
            })
            .map(|entry| self.build_match(entry, now))
            .collect();
        Ok(matches)
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Copy { text } => {
                self.clipboard.write_text(text).await?;
                Ok(ActionOutcome {
                    message: Some("Copied".to_string()),
                })
            }
            Action::Custom { kind, payload } if kind == "clipboard" => {
                self.invoke_custom(payload).await
            }
            _ => Err(DomainError::Unsupported(
                "unsupported clipboard action".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantum_domain::ClipboardData;
    use std::sync::Mutex as StdMutex;

    /// A store fake recording remove/clear calls and serving a fixed entry set.
    struct FakeStore {
        entries: StdMutex<Vec<ClipboardEntry>>,
        removed: StdMutex<Vec<String>>,
        cleared: StdMutex<usize>,
    }

    impl FakeStore {
        fn new(entries: Vec<ClipboardEntry>) -> Arc<Self> {
            Arc::new(Self {
                entries: StdMutex::new(entries),
                removed: StdMutex::new(Vec::new()),
                cleared: StdMutex::new(0),
            })
        }
    }

    #[async_trait]
    impl ClipboardStore for FakeStore {
        async fn load(&self) -> Result<ClipboardData, ClipboardError> {
            Ok(ClipboardData {
                entries: self.entries.lock().expect("entries").clone(),
            })
        }
        async fn append(
            &self,
            entry: ClipboardEntry,
            _blob: Option<Vec<u8>>,
        ) -> Result<(), ClipboardError> {
            self.entries.lock().expect("entries").push(entry);
            Ok(())
        }
        async fn remove(&self, id: &str) -> Result<(), ClipboardError> {
            self.removed.lock().expect("removed").push(id.to_string());
            Ok(())
        }
        async fn clear(&self) -> Result<(), ClipboardError> {
            *self.cleared.lock().expect("cleared") += 1;
            Ok(())
        }
    }

    struct FakeClipboard {
        texts: StdMutex<Vec<String>>,
    }

    impl FakeClipboard {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                texts: StdMutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl ClipboardWriter for FakeClipboard {
        async fn write_text(&self, text: &str) -> Result<(), DomainError> {
            self.texts.lock().expect("texts").push(text.to_string());
            Ok(())
        }
        async fn write_bytes(&self, _mime: &str, _bytes: &[u8]) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn text(id: &str, created: u64, full: &str) -> ClipboardEntry {
        ClipboardEntry::Text {
            id: id.to_string(),
            created_unix: created,
            size_bytes: full.len() as u64,
            preview: full.to_string(),
            full: full.to_string(),
        }
    }

    #[tokio::test]
    async fn semicolon_empty_returns_all_newest_first() {
        let store = FakeStore::new(vec![
            text("a", 100, "alpha"),
            text("b", 300, "bravo"),
            text("c", 200, "charlie"),
        ]);
        let provider = ClipboardProvider::new(store, FakeClipboard::new());
        let matches = provider.search(&Query::new(";")).await.unwrap();
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].id, "b");
        assert_eq!(matches[1].id, "c");
        assert_eq!(matches[2].id, "a");
    }

    #[tokio::test]
    async fn semicolon_query_filters_text() {
        let store = FakeStore::new(vec![
            text("a", 100, "hello world"),
            text("b", 200, "goodbye"),
        ]);
        let provider = ClipboardProvider::new(store, FakeClipboard::new());
        let matches = provider.search(&Query::new(";good")).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "b");
    }

    #[tokio::test]
    async fn non_semicolon_query_returns_zero() {
        let store = FakeStore::new(vec![text("a", 100, "hello")]);
        let provider = ClipboardProvider::new(store, FakeClipboard::new());
        let matches = provider.search(&Query::new("hello")).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn invoke_copy_writes_text() {
        let store = FakeStore::new(vec![]);
        let clipboard = FakeClipboard::new();
        let provider = ClipboardProvider::new(store, clipboard.clone());
        provider
            .invoke(&Action::Copy {
                text: "paste me".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(
            clipboard.texts.lock().expect("texts").as_slice(),
            ["paste me".to_string()]
        );
    }

    #[tokio::test]
    async fn invoke_delete_calls_store_remove() {
        let store = FakeStore::new(vec![text("a", 100, "hello")]);
        let provider = ClipboardProvider::new(store.clone(), FakeClipboard::new());
        provider
            .invoke(&custom_op("delete", Some("a")))
            .await
            .unwrap();
        assert_eq!(
            store.removed.lock().expect("removed").as_slice(),
            ["a".to_string()]
        );
    }

    #[tokio::test]
    async fn invoke_clear_calls_store_clear() {
        let store = FakeStore::new(vec![text("a", 100, "hello")]);
        let provider = ClipboardProvider::new(store.clone(), FakeClipboard::new());
        provider.invoke(&custom_op("clear", None)).await.unwrap();
        assert_eq!(*store.cleared.lock().expect("cleared"), 1);
    }

    #[tokio::test]
    async fn invoke_recopy_text_writes_full_text() {
        let store = FakeStore::new(vec![text("a", 100, "full text here")]);
        let clipboard = FakeClipboard::new();
        let provider = ClipboardProvider::new(store, clipboard.clone());
        provider
            .invoke(&custom_op("recopy", Some("a")))
            .await
            .unwrap();
        assert_eq!(
            clipboard.texts.lock().expect("texts").as_slice(),
            ["full text here".to_string()]
        );
    }

    #[test]
    fn relative_age_buckets() {
        assert_eq!(relative_age(100, 100), "just now");
        assert_eq!(relative_age(100, 100 + 30), "just now");
        assert_eq!(relative_age(100, 100 + 120), "2m ago");
        assert_eq!(relative_age(100, 100 + 7200), "2h ago");
        assert_eq!(relative_age(100, 100 + 2 * 86400), "2d ago");
    }

    #[test]
    fn single_line_collapses_and_caps() {
        assert_eq!(single_line("a   b\n c"), "a b c");
        let long: String = "x".repeat(200);
        let line = single_line(&long);
        assert!(line.chars().count() <= 81);
        assert!(line.ends_with('…'));
    }
}
