//! Pinned-location persistence for the file explorer sidebar.
//!
//! A single writable JSON file lists the user's pinned folders. Writes mirror
//! the timer store's atomic pattern: serialize to a sibling temporary file in
//! the same directory, then rename it over the target, so a crash mid-write can
//! never leave a partially written or corrupt list on disk. Loading never
//! errors; a missing or unparseable file yields the built-in defaults.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
pub use quantum_domain::Pin;
use quantum_domain::{FilesError, PinsPort};

/// A writable JSON store of pinned folders backed by a single file on disk.
pub struct PinStore {
    path: PathBuf,
}

impl PinStore {
    /// Construct a store backed by an explicit file path. Tests point this at a
    /// temporary directory; production wiring uses [`default_store_path`].
    pub fn new(store_path: PathBuf) -> Self {
        Self { path: store_path }
    }

    /// Load the pinned folders. Reads and parses the JSON array at the store
    /// path. A missing file yields the defaults silently; an unparseable file
    /// logs a warning and also yields the defaults. This never errors.
    pub fn load(&self) -> Vec<Pin> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(_) => return default_pins(&home_directory()),
        };

        match serde_json::from_str::<Vec<Pin>>(&contents) {
            Ok(pins) => pins,
            Err(error) => {
                tracing::warn!(
                    path = %self.path.display(),
                    %error,
                    "failed to parse pin store; falling back to defaults"
                );
                default_pins(&home_directory())
            }
        }
    }

    /// Persist the pinned folders atomically. Creates the parent directory when
    /// missing, serializes to pretty JSON, writes a sibling temporary file, and
    /// renames it over the target. Input/output failures map to
    /// [`FilesError::Io`].
    pub fn save(&self, pins: &[Pin]) -> Result<(), FilesError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                FilesError::Io(format!("failed to create {}: {error}", parent.display()))
            })?;
        }

        let serialized = serde_json::to_string_pretty(pins)
            .map_err(|error| FilesError::Io(format!("failed to serialize pin store: {error}")))?;

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

    /// Add a pin, appending it only when no existing pin shares its path, then
    /// persist. Returns the resulting list.
    pub fn add(&self, pin: Pin) -> Result<Vec<Pin>, FilesError> {
        let mut pins = self.load();
        if !pins.iter().any(|existing| existing.path == pin.path) {
            pins.push(pin);
        }
        self.save(&pins)?;
        Ok(pins)
    }

    /// Remove every pin matching the given path, then persist. Returns the
    /// resulting list.
    pub fn remove(&self, path: &str) -> Result<Vec<Pin>, FilesError> {
        let mut pins = self.load();
        pins.retain(|pin| pin.path != path);
        self.save(&pins)?;
        Ok(pins)
    }
}

/// Adapt the synchronous [`PinStore`] to the asynchronous domain [`PinsPort`].
///
/// Each mutating method returns the full pin list after the change so the
/// application layer can broadcast the new state without a second read. The
/// underlying store performs quick, local file input/output, so the async
/// methods call it directly rather than offloading to a blocking thread.
#[async_trait]
impl PinsPort for PinStore {
    async fn load(&self) -> Vec<Pin> {
        PinStore::load(self)
    }

    async fn add(&self, pin: Pin) -> Result<Vec<Pin>, FilesError> {
        PinStore::add(self, pin)
    }

    async fn remove(&self, path: &str) -> Result<Vec<Pin>, FilesError> {
        PinStore::remove(self, path)
    }
}

/// Resolve the default persistence path:
/// `$XDG_STATE_HOME/quantum/files.json`, falling back to
/// `$HOME/.local/state/quantum/files.json` when `XDG_STATE_HOME` is unset.
/// Mirrors the timer store's path resolution.
pub fn default_store_path() -> PathBuf {
    let state_home = std::env::var("XDG_STATE_HOME")
        .unwrap_or_else(|_| format!("{}/.local/state", home_directory()));

    PathBuf::from(state_home).join("quantum/files.json")
}

/// Read `$HOME`, falling back to an empty string when it is unset.
fn home_directory() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// The default pinned folders for a given home directory, keeping only those
/// that exist on disk. A thin wrapper over [`default_pins_with`] using a real
/// [`Path::exists`] check.
pub fn default_pins(home: &str) -> Vec<Pin> {
    default_pins_with(home, &|path| Path::new(path).exists())
}

/// The default pinned folders using an injectable existence check. Pure and
/// unit-testable: given `home` and an `exists` predicate, it returns Home,
/// Documents, Downloads, and Pictures in that order, filtered to those the
/// predicate accepts.
pub fn default_pins_with(home: &str, exists: &dyn Fn(&str) -> bool) -> Vec<Pin> {
    let candidates = [
        ("Home", home.to_string()),
        ("Documents", format!("{home}/Documents")),
        ("Downloads", format!("{home}/Downloads")),
        ("Pictures", format!("{home}/Pictures")),
    ];

    candidates
        .into_iter()
        .filter(|(_, path)| exists(path))
        .map(|(label, path)| Pin {
            label: label.to_string(),
            path,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(label: &str, path: &str) -> Pin {
        Pin {
            label: label.to_string(),
            path: path.to_string(),
        }
    }

    #[test]
    fn roundtrip_save_then_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("files.json");
        let store = PinStore::new(path.clone());
        let pins = vec![
            pin("Home", "/home/user"),
            pin("Projects", "/home/user/Projects"),
        ];
        store.save(&pins).unwrap();
        assert!(path.exists());
        assert_eq!(store.load(), pins);
    }

    #[test]
    fn missing_file_load_returns_defaults_for_existing_dirs() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("Documents")).unwrap();
        std::fs::create_dir(home.path().join("Downloads")).unwrap();
        // Pictures is intentionally not created.

        let previous_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", home.path());
        let store = PinStore::new(home.path().join("nonexistent").join("files.json"));
        let pins = store.load();
        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }

        let labels: Vec<&str> = pins.iter().map(|entry| entry.label.as_str()).collect();
        assert!(labels.contains(&"Home"), "got {labels:?}");
        assert!(labels.contains(&"Documents"), "got {labels:?}");
        assert!(labels.contains(&"Downloads"), "got {labels:?}");
        assert!(!labels.contains(&"Pictures"), "got {labels:?}");
    }

    #[test]
    fn corrupt_file_load_returns_defaults_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("files.json");
        std::fs::write(&path, b"not json{").unwrap();
        let store = PinStore::new(path);
        // Must not panic; returns a usable list rather than erroring.
        let _pins = store.load();
    }

    #[test]
    fn default_pins_with_filters_by_existence_in_order() {
        let home = "/home/user";
        let exists = |path: &str| path == "/home/user" || path == "/home/user/Documents";
        let pins = default_pins_with(home, &exists);
        assert_eq!(
            pins,
            vec![
                pin("Home", "/home/user"),
                pin("Documents", "/home/user/Documents")
            ]
        );
    }

    #[test]
    fn add_is_idempotent_by_path_and_remove_drops() {
        let dir = tempfile::tempdir().unwrap();
        let store = PinStore::new(dir.path().join("files.json"));
        store.save(&[]).unwrap();

        let entry = pin("Projects", "/home/user/Projects");
        store.add(entry.clone()).unwrap();
        let after_second = store.add(entry.clone()).unwrap();
        assert_eq!(after_second, vec![entry.clone()]);

        let after_remove = store.remove("/home/user/Projects").unwrap();
        assert!(after_remove.is_empty());
    }

    #[tokio::test]
    async fn pins_port_add_load_remove_roundtrip_returns_domain_pin() {
        let dir = tempfile::tempdir().unwrap();
        let store = PinStore::new(dir.path().join("files.json"));
        store.save(&[]).unwrap();

        let entry = pin("Projects", "/home/user/Projects");
        let after_add = PinsPort::add(&store, entry.clone()).await.unwrap();
        // The value returned by the async trait is the domain Pin type.
        let returned: quantum_domain::Pin = after_add[0].clone();
        assert_eq!(returned, entry);

        let loaded = PinsPort::load(&store).await;
        assert_eq!(loaded, vec![entry.clone()]);

        let after_remove = PinsPort::remove(&store, "/home/user/Projects")
            .await
            .unwrap();
        assert!(after_remove.is_empty());
    }

    #[test]
    fn save_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("files.json");
        let store = PinStore::new(path);
        store.save(&[pin("Home", "/home/user")]).unwrap();

        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["files.json".to_string()]);
    }
}
