//! Writable JSON persistence for the timer subsystem.
//!
//! Implements the [`quantum_domain::TimerStore`] port against a file under
//! `$XDG_STATE_HOME/quantum/timers.json`, writing atomically via a temp file
//! and rename so a crash mid-write cannot corrupt the persisted state.

use std::path::PathBuf;

use async_trait::async_trait;
use quantum_domain::{TimerError, TimerStore, TimerStoreData};

/// A [`TimerStore`] backed by a single JSON file on disk.
pub struct JsonTimerStore {
    path: PathBuf,
}

impl JsonTimerStore {
    /// Construct a store using the default XDG state path.
    pub fn new() -> Self {
        Self {
            path: Self::default_path(),
        }
    }

    /// Construct a store using an explicit path. Primarily for tests.
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Resolve the default persistence path:
    /// `$XDG_STATE_HOME/quantum/timers.json`, falling back to
    /// `~/.local/state/quantum/timers.json` when `XDG_STATE_HOME` is unset.
    fn default_path() -> PathBuf {
        let state_home = std::env::var("XDG_STATE_HOME")
            .unwrap_or_else(|_| format!("{}/.local/state", std::env::var("HOME").unwrap_or_default()));

        PathBuf::from(state_home).join("quantum/timers.json")
    }
}

impl Default for JsonTimerStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TimerStore for JsonTimerStore {
    async fn load(&self) -> Result<TimerStoreData, TimerError> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(TimerStoreData::default());
            }
            Err(error) => {
                return Err(TimerError::Persistence(format!(
                    "failed to read {}: {error}",
                    self.path.display()
                )));
            }
        };

        match serde_json::from_str::<TimerStoreData>(&contents) {
            Ok(data) => Ok(data),
            Err(error) => {
                tracing::warn!(
                    path = %self.path.display(),
                    %error,
                    "failed to parse timer store; falling back to defaults"
                );
                Ok(TimerStoreData::default())
            }
        }
    }

    async fn save(&self, data: &TimerStoreData) -> Result<(), TimerError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                TimerError::Persistence(format!(
                    "failed to create {}: {error}",
                    parent.display()
                ))
            })?;
        }

        let serialized = serde_json::to_string_pretty(data).map_err(|error| {
            TimerError::Persistence(format!("failed to serialize timer store: {error}"))
        })?;

        let temp_path = self.path.with_extension("tmp");
        std::fs::write(&temp_path, serialized.as_bytes()).map_err(|error| {
            TimerError::Persistence(format!(
                "failed to write {}: {error}",
                temp_path.display()
            ))
        })?;

        std::fs::rename(&temp_path, &self.path).map_err(|error| {
            TimerError::Persistence(format!(
                "failed to rename {} to {}: {error}",
                temp_path.display(),
                self.path.display()
            ))
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantum_domain::{
        NotifyConfig, TimeOfDay, Timer, TimerId, TimerKind, TimerStatus, VisualConfig, Weekday,
        WeekdaySet,
    };

    fn one_shot() -> Timer {
        Timer {
            id: TimerId::from("t1"),
            label: "Tea".to_string(),
            kind: TimerKind::OneShot { end_unix: 1700 },
            visual: VisualConfig::default(),
            notify: NotifyConfig::default(),
            status: TimerStatus::Active,
            scatter_pos: None,
        }
    }

    fn recurring() -> Timer {
        Timer {
            id: TimerId::from("t2"),
            label: "Standup".to_string(),
            kind: TimerKind::Recurring {
                days: WeekdaySet::from_days(&[Weekday::Monday, Weekday::Friday]),
                time: TimeOfDay::new(9, 0).unwrap(),
                next_fire_unix: 9000,
            },
            visual: VisualConfig::default(),
            notify: NotifyConfig::default(),
            status: TimerStatus::Expired,
            scatter_pos: None,
        }
    }

    #[tokio::test]
    async fn missing_file_loads_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonTimerStore::with_path(dir.path().join("timers.json"));
        let data = store.load().await.unwrap();
        assert!(data.timers.is_empty());
    }

    #[tokio::test]
    async fn roundtrip_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timers.json");
        let store = JsonTimerStore::with_path(path.clone());
        let mut data = TimerStoreData::default();
        data.timers.push(one_shot());
        data.timers.push(recurring());
        store.save(&data).await.unwrap();
        assert!(path.exists());
        let back = store.load().await.unwrap();
        assert_eq!(back, data);
    }

    #[tokio::test]
    async fn corrupt_file_loads_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timers.json");
        std::fs::write(&path, b"not json").unwrap();
        let store = JsonTimerStore::with_path(path);
        assert!(store.load().await.unwrap().timers.is_empty());
    }

    #[tokio::test]
    async fn save_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/timers.json");
        let store = JsonTimerStore::with_path(path.clone());
        store.save(&TimerStoreData::default()).await.unwrap();
        assert!(path.exists());
    }
}
