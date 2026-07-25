//! Persisted file-explorer preferences.
//!
//! A single writable JSON file holds the user's explorer preferences. Writes
//! mirror the pin store's atomic pattern: serialize to a sibling temporary file
//! in the same directory, then rename it over the target, so a crash mid-write
//! can never leave a partially written or corrupt file on disk. Loading never
//! errors; a missing or unparseable file yields [`FilePreferences::default`].

use std::path::PathBuf;

use async_trait::async_trait;
use quantum_domain::{FilePreferences, FilesError, PreferencesPort};

/// A writable JSON store of file-explorer preferences backed by a single file
/// on disk.
pub struct PreferencesStore {
    path: PathBuf,
}

impl PreferencesStore {
    /// Construct a store backed by an explicit file path. Tests point this at a
    /// temporary directory; production wiring uses
    /// [`preferences_default_store_path`].
    pub fn new(store_path: PathBuf) -> Self {
        Self { path: store_path }
    }

    /// Load the preferences. Reads and parses the JSON object at the store
    /// path. A missing file yields the defaults silently; an unparseable file
    /// logs a warning and also yields the defaults. This never errors.
    pub fn load(&self) -> FilePreferences {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(_) => return FilePreferences::default(),
        };

        match serde_json::from_str::<FilePreferences>(&contents) {
            Ok(preferences) => preferences,
            Err(error) => {
                tracing::warn!(
                    path = %self.path.display(),
                    %error,
                    "failed to parse preferences store; falling back to defaults"
                );
                FilePreferences::default()
            }
        }
    }

    /// Persist the preferences atomically. Creates the parent directory when
    /// missing, serializes to pretty JSON, writes a sibling temporary file, and
    /// renames it over the target. Input/output failures map to
    /// [`FilesError::Io`].
    pub fn save(&self, preferences: &FilePreferences) -> Result<(), FilesError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                FilesError::Io(format!("failed to create {}: {error}", parent.display()))
            })?;
        }

        let serialized = serde_json::to_string_pretty(preferences).map_err(|error| {
            FilesError::Io(format!("failed to serialize preferences store: {error}"))
        })?;

        // A fixed `.tmp` sibling mirrors the pin and timer stores' established
        // pattern. Both assume a single writer: `quantumd` owns the file and no
        // two processes write it concurrently, so a fixed name cannot race.
        let temp_path = self.path.with_extension("tmp");
        std::fs::write(&temp_path, serialized.as_bytes()).map_err(|error| {
            FilesError::Io(format!("failed to write {}: {error}", temp_path.display()))
        })?;

        std::fs::rename(&temp_path, &self.path).map_err(|error| {
            FilesError::Io(format!(
                "failed to rename {} to {}: {error}",
                temp_path.display(),
                self.path.display()
            ))
        })?;

        Ok(())
    }
}

/// Adapt the synchronous [`PreferencesStore`] to the asynchronous domain
/// [`PreferencesPort`]. The underlying store performs quick, local file
/// input/output, so the async methods call it directly rather than offloading
/// to a blocking thread.
#[async_trait]
impl PreferencesPort for PreferencesStore {
    async fn load(&self) -> FilePreferences {
        PreferencesStore::load(self)
    }

    async fn save(&self, preferences: FilePreferences) -> Result<(), FilesError> {
        PreferencesStore::save(self, &preferences)
    }
}

/// Resolve the default persistence path:
/// `$XDG_STATE_HOME/quantum/files-preferences.json`, falling back to
/// `$HOME/.local/state/quantum/files-preferences.json` when `XDG_STATE_HOME` is
/// unset. Mirrors the pin store's path resolution.
pub fn preferences_default_store_path() -> PathBuf {
    let state_home = std::env::var("XDG_STATE_HOME")
        .unwrap_or_else(|_| format!("{}/.local/state", home_directory()));

    PathBuf::from(state_home).join("quantum/files-preferences.json")
}

/// Read `$HOME`, falling back to an empty string when it is unset.
fn home_directory() -> String {
    std::env::var("HOME").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantum_domain::FilePreferences;

    #[test]
    fn roundtrip_save_then_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreferencesStore::new(dir.path().join("files-preferences.json"));
        store
            .save(&FilePreferences {
                show_hidden: false,
                pinned_actions: Vec::new(),
            })
            .unwrap();
        assert_eq!(
            store.load(),
            FilePreferences {
                show_hidden: false,
                pinned_actions: Vec::new(),
            }
        );
    }

    #[test]
    fn missing_file_load_returns_default_true() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreferencesStore::new(dir.path().join("nope.json"));
        assert!(store.load().show_hidden);
    }

    #[test]
    fn corrupt_file_load_returns_default_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("files-preferences.json");
        std::fs::write(&path, b"not json{").unwrap();
        let store = PreferencesStore::new(path);
        assert!(store.load().show_hidden);
    }

    #[test]
    fn save_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreferencesStore::new(dir.path().join("files-preferences.json"));
        store.save(&FilePreferences::default()).unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["files-preferences.json".to_string()]);
    }
}
