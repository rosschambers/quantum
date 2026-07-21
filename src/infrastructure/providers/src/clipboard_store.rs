//! Writable blob-plus-JSON persistence for the clipboard-history subsystem.
//!
//! Implements the [`quantum_domain::ClipboardStore`] port. JSON metadata lives
//! at `$XDG_STATE_HOME/quantum/clipboard.json`, written atomically via a temp
//! file plus rename so a crash mid-write cannot corrupt it (the
//! [`crate::timer_store::JsonTimerStore`] pattern). Blob-backed entries (image,
//! binary) additionally store their bytes at
//! `$XDG_STATE_HOME/quantum/clipboard/<id>.bin`.
//!
//! Ordering matters: [`FileClipboardStore::append`] writes the blob before the
//! JSON row, so a crash between the two leaves an orphan blob (garbage-collected
//! on the next [`FileClipboardStore::load`]) rather than a row that references
//! missing bytes.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quantum_domain::{ClipboardData, ClipboardEntry, ClipboardError, ClipboardStore};

/// Maximum number of entries retained. Appending past this evicts the oldest.
const MAX_ENTRIES: usize = 100;

/// Maximum total size, in bytes, of all retained blobs. Appending past this
/// evicts the oldest entries until the total is back under the cap.
const MAX_TOTAL_BLOB_BYTES: u64 = 256 * 1024 * 1024;

/// Maximum size, in bytes, of a single entry. An entry larger than this is
/// rejected on append (not stored) rather than evicting the entire history to
/// make room for it.
const MAX_SINGLE_ENTRY: u64 = 32 * 1024 * 1024;

/// A [`ClipboardStore`] backed by a JSON metadata file plus a sibling blob
/// directory on disk.
pub struct FileClipboardStore {
    json_path: PathBuf,
    blob_dir: PathBuf,
}

impl FileClipboardStore {
    /// Construct a store using the default XDG state paths.
    pub fn new() -> Self {
        let (json_path, blob_dir) = Self::default_paths();
        Self {
            json_path,
            blob_dir,
        }
    }

    /// Construct a store using explicit paths. Primarily for tests.
    pub fn with_paths(json_path: PathBuf, blob_dir: PathBuf) -> Self {
        Self {
            json_path,
            blob_dir,
        }
    }

    /// The directory this store writes blob files to. The clipboard watcher
    /// needs it to fill each blob-backed entry's `blob_path`.
    pub fn blob_directory(&self) -> &Path {
        &self.blob_dir
    }

    /// Resolve the default persistence paths:
    /// `$XDG_STATE_HOME/quantum/clipboard.json` and
    /// `$XDG_STATE_HOME/quantum/clipboard/`, falling back to
    /// `~/.local/state/quantum/...` when `XDG_STATE_HOME` is unset.
    fn default_paths() -> (PathBuf, PathBuf) {
        let state_home = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            format!("{}/.local/state", std::env::var("HOME").unwrap_or_default())
        });
        let base = PathBuf::from(state_home).join("quantum");
        (base.join("clipboard.json"), base.join("clipboard"))
    }

    /// The on-disk blob path for an entry id.
    fn blob_path_for(&self, id: &str) -> PathBuf {
        self.blob_dir.join(format!("{id}.bin"))
    }

    /// Read and parse the JSON metadata without any garbage collection. A
    /// missing file yields an empty [`ClipboardData`]; a corrupt file yields an
    /// empty [`ClipboardData`] and a warning.
    fn read_raw(&self) -> ClipboardData {
        let contents = match std::fs::read_to_string(&self.json_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return ClipboardData::default();
            }
            Err(error) => {
                tracing::warn!(
                    path = %self.json_path.display(),
                    %error,
                    "failed to read clipboard store; falling back to empty"
                );
                return ClipboardData::default();
            }
        };

        match serde_json::from_str::<ClipboardData>(&contents) {
            Ok(data) => data,
            Err(error) => {
                tracing::warn!(
                    path = %self.json_path.display(),
                    %error,
                    "failed to parse clipboard store; falling back to empty"
                );
                ClipboardData::default()
            }
        }
    }

    /// Serialize `data` to the JSON metadata file atomically (temp file plus
    /// rename), creating parent directories as needed.
    fn write_json(&self, data: &ClipboardData) -> Result<(), ClipboardError> {
        if let Some(parent) = self.json_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                tracing::warn!(path = %parent.display(), %error, "failed to create clipboard state directory");
                ClipboardError::Persistence(format!("failed to create {}: {error}", parent.display()))
            })?;
        }

        let serialized = serde_json::to_string_pretty(data).map_err(|error| {
            tracing::warn!(%error, "failed to serialize clipboard store");
            ClipboardError::Persistence(format!("failed to serialize clipboard store: {error}"))
        })?;

        let temp_path = self.json_path.with_extension("tmp");
        std::fs::write(&temp_path, serialized.as_bytes()).map_err(|error| {
            tracing::warn!(path = %temp_path.display(), %error, "failed to write clipboard temp file");
            ClipboardError::Persistence(format!("failed to write {}: {error}", temp_path.display()))
        })?;

        std::fs::rename(&temp_path, &self.json_path).map_err(|error| {
            tracing::warn!(
                from = %temp_path.display(),
                to = %self.json_path.display(),
                %error,
                "failed to rename clipboard temp file into place"
            );
            ClipboardError::Persistence(format!(
                "failed to rename {} to {}: {error}",
                temp_path.display(),
                self.json_path.display()
            ))
        })?;

        Ok(())
    }

    /// Delete the blob file for `id`, if present. A missing blob is not an
    /// error; any other input/output failure is logged but not propagated so a
    /// single stuck blob cannot wedge a remove or clear.
    fn delete_blob(&self, id: &str) {
        let path = self.blob_path_for(id);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "failed to delete clipboard blob");
            }
        }
    }
}

impl Default for FileClipboardStore {
    fn default() -> Self {
        Self::new()
    }
}

/// True when `path`'s file name matches the `<id>.bin` blob naming scheme,
/// returning the id. Used by the load-time orphan sweep.
fn blob_id_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    name.strip_suffix(".bin").map(|id| id.to_string())
}

#[async_trait]
impl ClipboardStore for FileClipboardStore {
    async fn load(&self) -> Result<ClipboardData, ClipboardError> {
        let mut data = self.read_raw();

        // Drop rows whose blob file is missing (blob-backed kinds only), so a
        // deleted-out-of-band blob does not surface as a broken entry.
        let mut live_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        data.entries.retain(|entry| match entry.blob_path() {
            None => {
                live_ids.insert(entry.id().to_string());
                true
            }
            Some(_) => {
                let present = self.blob_path_for(entry.id()).exists();
                if present {
                    live_ids.insert(entry.id().to_string());
                } else {
                    tracing::warn!(
                        id = %entry.id(),
                        "dropping clipboard row whose blob file is missing"
                    );
                }
                present
            }
        });

        // Garbage-collect orphan blobs: any `<id>.bin` with no matching row.
        match std::fs::read_dir(&self.blob_dir) {
            Ok(reader) => {
                for entry in reader.flatten() {
                    let path = entry.path();
                    if let Some(id) = blob_id_from_path(&path) {
                        if !live_ids.contains(&id) {
                            if let Err(error) = std::fs::remove_file(&path) {
                                tracing::warn!(
                                    path = %path.display(),
                                    %error,
                                    "failed to delete orphan clipboard blob"
                                );
                            }
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(
                    path = %self.blob_dir.display(),
                    %error,
                    "failed to scan clipboard blob directory for orphans"
                );
            }
        }

        Ok(data)
    }

    async fn append(
        &self,
        entry: ClipboardEntry,
        blob: Option<Vec<u8>>,
    ) -> Result<(), ClipboardError> {
        // Reject an oversized single entry rather than evicting the whole
        // history to make room for it. Not storing is not an error.
        if entry.size_bytes() > MAX_SINGLE_ENTRY {
            tracing::warn!(
                id = %entry.id(),
                size_bytes = entry.size_bytes(),
                "rejecting oversized clipboard entry"
            );
            return Ok(());
        }

        // Write the blob FIRST (blob-backed kinds only), then the JSON row, so a
        // crash between the two leaves a garbage-collectable orphan blob rather
        // than a row referencing missing bytes.
        if let Some(bytes) = blob.as_ref() {
            std::fs::create_dir_all(&self.blob_dir).map_err(|error| {
                tracing::warn!(path = %self.blob_dir.display(), %error, "failed to create clipboard blob directory");
                ClipboardError::Persistence(format!(
                    "failed to create {}: {error}",
                    self.blob_dir.display()
                ))
            })?;
            let path = self.blob_path_for(entry.id());
            std::fs::write(&path, bytes).map_err(|error| {
                tracing::warn!(path = %path.display(), %error, "failed to write clipboard blob");
                ClipboardError::Persistence(format!("failed to write {}: {error}", path.display()))
            })?;
        }

        let mut data = self.read_raw();
        data.entries.push(entry);

        // Evict oldest (front of the list) until under both caps. Deleting a
        // row's blob keeps the blob directory in step with the metadata.
        while data.entries.len() > MAX_ENTRIES
            || data
                .entries
                .iter()
                .map(|entry| entry.size_bytes())
                .sum::<u64>()
                > MAX_TOTAL_BLOB_BYTES
        {
            if data.entries.is_empty() {
                break;
            }
            let evicted = data.entries.remove(0);
            self.delete_blob(evicted.id());
        }

        self.write_json(&data)
    }

    async fn remove(&self, id: &str) -> Result<(), ClipboardError> {
        let mut data = self.read_raw();
        let before = data.entries.len();
        data.entries.retain(|entry| entry.id() != id);
        if data.entries.len() != before {
            self.delete_blob(id);
        }
        self.write_json(&data)
    }

    async fn clear(&self) -> Result<(), ClipboardError> {
        let data = self.read_raw();
        for entry in &data.entries {
            self.delete_blob(entry.id());
        }
        // Also sweep any stray blobs that were not referenced by a row.
        match std::fs::read_dir(&self.blob_dir) {
            Ok(reader) => {
                for entry in reader.flatten() {
                    let path = entry.path();
                    if blob_id_from_path(&path).is_some() {
                        if let Err(error) = std::fs::remove_file(&path) {
                            tracing::warn!(
                                path = %path.display(),
                                %error,
                                "failed to delete clipboard blob during clear"
                            );
                        }
                    }
                }
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!(
                    path = %self.blob_dir.display(),
                    %error,
                    "failed to read clipboard blob directory during clear"
                );
            }
            Err(_) => {}
        }
        self.write_json(&ClipboardData::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &std::path::Path) -> FileClipboardStore {
        FileClipboardStore::with_paths(dir.join("clipboard.json"), dir.join("clipboard"))
    }

    fn text(id: &str, created: u64) -> ClipboardEntry {
        ClipboardEntry::Text {
            id: id.to_string(),
            created_unix: created,
            size_bytes: 5,
            preview: "hello".to_string(),
            full: "hello".to_string(),
        }
    }

    fn image(id: &str, created: u64, size: u64, blob_dir: &std::path::Path) -> ClipboardEntry {
        ClipboardEntry::Image {
            id: id.to_string(),
            created_unix: created,
            size_bytes: size,
            preview_thumb: "data:image/png;base64,AAAA".to_string(),
            blob_path: blob_dir
                .join(format!("{id}.bin"))
                .to_string_lossy()
                .to_string(),
            width: 10,
            height: 10,
        }
    }

    #[tokio::test]
    async fn missing_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let data = store(dir.path()).load().await.unwrap();
        assert!(data.entries.is_empty());
    }

    #[tokio::test]
    async fn corrupt_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clipboard.json"), b"not json").unwrap();
        let data = store(dir.path()).load().await.unwrap();
        assert!(data.entries.is_empty());
    }

    #[tokio::test]
    async fn round_trip_text_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store.append(text("c1", 100), None).await.unwrap();
        let data = store.load().await.unwrap();
        assert_eq!(data.entries.len(), 1);
        assert_eq!(data.entries[0].id(), "c1");
    }

    #[tokio::test]
    async fn round_trip_image_entry_with_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let blob_dir = dir.path().join("clipboard");
        let entry = image("c2", 200, 4, &blob_dir);
        store.append(entry, Some(vec![1, 2, 3, 4])).await.unwrap();

        // The blob file exists beside the JSON.
        assert!(blob_dir.join("c2.bin").exists());
        let bytes = std::fs::read(blob_dir.join("c2.bin")).unwrap();
        assert_eq!(bytes, vec![1, 2, 3, 4]);

        let data = store.load().await.unwrap();
        assert_eq!(data.entries.len(), 1);
        assert_eq!(data.entries[0].id(), "c2");
    }

    #[tokio::test]
    async fn append_writes_blob_before_json() {
        // With an unwritable JSON path (a directory where the file should be),
        // the JSON write fails, but the blob must already have been written,
        // proving the blob-first ordering.
        let dir = tempfile::tempdir().unwrap();
        let json_as_dir = dir.path().join("clipboard.json");
        std::fs::create_dir(&json_as_dir).unwrap();
        let blob_dir = dir.path().join("clipboard");
        let store = FileClipboardStore::with_paths(json_as_dir, blob_dir.clone());

        let entry = image("c9", 1, 3, &blob_dir);
        let result = store.append(entry, Some(vec![9, 9, 9])).await;

        // The JSON rename cannot overwrite a directory, so append reports an
        // error, yet the blob was written first.
        assert!(result.is_err());
        assert!(blob_dir.join("c9.bin").exists());
    }

    #[tokio::test]
    async fn load_garbage_collects_orphan_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let blob_dir = dir.path().join("clipboard");
        std::fs::create_dir_all(&blob_dir).unwrap();
        // An orphan blob with no matching row.
        std::fs::write(blob_dir.join("ghost.bin"), b"orphan").unwrap();
        // A real text row (no blob).
        store.append(text("c1", 10), None).await.unwrap();

        let data = store.load().await.unwrap();
        assert_eq!(data.entries.len(), 1);
        assert!(!blob_dir.join("ghost.bin").exists());
    }

    #[tokio::test]
    async fn load_drops_row_with_missing_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let blob_dir = dir.path().join("clipboard");
        // Persist an image row but never write its blob.
        let entry = image("c5", 1, 4, &blob_dir);
        // Append with the bytes so the row persists, then delete the blob.
        store.append(entry, Some(vec![0; 4])).await.unwrap();
        std::fs::remove_file(blob_dir.join("c5.bin")).unwrap();

        let data = store.load().await.unwrap();
        assert!(data.entries.is_empty());
    }

    #[tokio::test]
    async fn append_evicts_oldest_past_max_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        for index in 0..(MAX_ENTRIES + 5) {
            store
                .append(text(&format!("c{index}"), index as u64), None)
                .await
                .unwrap();
        }
        let data = store.load().await.unwrap();
        assert_eq!(data.entries.len(), MAX_ENTRIES);
        // The five oldest were evicted; the newest remains.
        assert_eq!(data.entries[0].id(), "c5");
        assert_eq!(
            data.entries.last().unwrap().id(),
            &format!("c{}", MAX_ENTRIES + 4)
        );
    }

    #[tokio::test]
    async fn oversized_entry_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let blob_dir = dir.path().join("clipboard");
        let entry = image("big", 1, MAX_SINGLE_ENTRY + 1, &blob_dir);
        store.append(entry, Some(vec![0; 8])).await.unwrap();
        let data = store.load().await.unwrap();
        assert!(data.entries.is_empty());
    }

    #[tokio::test]
    async fn remove_drops_row_and_deletes_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let blob_dir = dir.path().join("clipboard");
        store
            .append(image("c2", 1, 4, &blob_dir), Some(vec![1, 2, 3, 4]))
            .await
            .unwrap();
        assert!(blob_dir.join("c2.bin").exists());

        store.remove("c2").await.unwrap();
        let data = store.load().await.unwrap();
        assert!(data.entries.is_empty());
        assert!(!blob_dir.join("c2.bin").exists());
    }

    #[tokio::test]
    async fn clear_empties_everything() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let blob_dir = dir.path().join("clipboard");
        store.append(text("c1", 1), None).await.unwrap();
        store
            .append(image("c2", 2, 4, &blob_dir), Some(vec![1, 2, 3, 4]))
            .await
            .unwrap();
        assert!(blob_dir.join("c2.bin").exists());

        store.clear().await.unwrap();
        let data = store.load().await.unwrap();
        assert!(data.entries.is_empty());
        assert!(!blob_dir.join("c2.bin").exists());
    }
}
