//! Persisted application launch-usage tracking.
//!
//! Records how often and how recently each desktop application is launched so
//! the launcher can show a usage-ranked set of default apps on an empty query.
//! State is a small JSON map persisted under `$XDG_DATA_HOME/quantum/` (or
//! `~/.local/share/quantum/`). A missing or corrupt file yields an empty store
//! rather than an error, so startup is never blocked by usage data.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Per-application usage record.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct UsageRecord {
    /// Total number of times this application has been launched.
    pub launch_count: u64,
    /// Unix timestamp (seconds) of the most recent launch.
    pub last_used: u64,
}

/// Persisted launch-usage store keyed by desktop id.
#[derive(Debug)]
pub struct UsageStore {
    path: PathBuf,
    records: HashMap<String, UsageRecord>,
}

/// Current unix time in seconds, saturating to 0 if the clock is before the
/// epoch (which should never happen on a real system).
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl UsageStore {
    /// Default on-disk location: `$XDG_DATA_HOME/quantum/app_usage.json`,
    /// falling back to `~/.local/share/quantum/app_usage.json`.
    fn default_path() -> PathBuf {
        let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            format!("{}/.local/share", std::env::var("HOME").unwrap_or_default())
        });
        PathBuf::from(data_home)
            .join("quantum")
            .join("app_usage.json")
    }

    /// Load the store from its default location.
    pub fn load() -> Self {
        Self::with_path(Self::default_path())
    }

    /// Load the store from an explicit path. A missing or unreadable/corrupt
    /// file yields an empty store; this never fails.
    pub fn with_path(path: PathBuf) -> Self {
        let records = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { path, records }
    }

    /// Record a launch of `desktop_id`: increment its count and stamp the
    /// current time, then persist. Persistence errors are returned so the
    /// caller can log them, but the in-memory state is always updated first.
    pub fn record(&mut self, desktop_id: &str) -> std::io::Result<()> {
        let entry = self.records.entry(desktop_id.to_string()).or_default();
        entry.launch_count = entry.launch_count.saturating_add(1);
        entry.last_used = now_secs();
        self.persist()
    }

    /// Score for a desktop id: frequency weighted by recency. Higher is more
    /// relevant. Unknown ids score 0.0. Recency uses a gentle decay so that,
    /// among equal launch counts, a more recently used app ranks higher.
    pub fn score(&self, desktop_id: &str) -> f64 {
        match self.records.get(desktop_id) {
            None => 0.0,
            Some(rec) => {
                let age_secs = now_secs().saturating_sub(rec.last_used) as f64;
                // Half-life of roughly one week (604800 seconds): an app used
                // a week ago counts about half as much as one used just now.
                let recency = 0.5_f64.powf(age_secs / 604_800.0);
                // Base on launch count so a frequently used app still ranks
                // well even if not the most recent; +1 keeps a single recent
                // launch meaningful.
                (rec.launch_count as f64) * recency
            }
        }
    }

    /// Rank `ids` by descending usage score. Ids with no usage history keep
    /// their original relative order and sort after all ids that do have
    /// history (a stable sort over a precomputed has-history flag + score).
    pub fn rank(&self, ids: &[String]) -> Vec<String> {
        let mut scored: Vec<(usize, &String, bool, f64)> = ids
            .iter()
            .enumerate()
            .map(|(idx, id)| {
                let known = self.records.contains_key(id);
                (idx, id, known, self.score(id))
            })
            .collect();
        scored.sort_by(|a, b| {
            // Known-before-unknown, then higher score first, then original
            // index to keep the sort stable and deterministic.
            b.2.cmp(&a.2)
                .then(b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.0.cmp(&b.0))
        });
        scored.into_iter().map(|(_, id, _, _)| id.clone()).collect()
    }

    /// Atomically write the store to disk: serialize to a sibling temp file in
    /// the same directory, flush, then rename over the target so a crash never
    /// leaves a half-written file.
    fn persist(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(&self.records).map_err(std::io::Error::other)?;
        let dir = self.path.parent().unwrap_or_else(|| Path::new("."));
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(&json)?;
        tmp.flush()?;
        tmp.persist(&self.path).map_err(std::io::Error::other)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, UsageStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app_usage.json");
        (dir, UsageStore::with_path(path))
    }

    #[test]
    fn missing_file_yields_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = UsageStore::with_path(dir.path().join("does-not-exist.json"));
        assert_eq!(store.score("firefox"), 0.0);
        assert!(store.rank(&["firefox".to_string()]).len() == 1);
    }

    #[test]
    fn corrupt_file_yields_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app_usage.json");
        std::fs::write(&path, b"this is not json {{{").unwrap();
        let store = UsageStore::with_path(path);
        assert_eq!(store.score("firefox"), 0.0);
    }

    #[test]
    fn record_increments_and_persists() {
        let (dir, mut store) = temp_store();
        store.record("firefox").unwrap();
        store.record("firefox").unwrap();

        // Reload from disk to prove persistence round-trips.
        let reloaded = UsageStore::with_path(dir.path().join("app_usage.json"));
        assert!(reloaded.score("firefox") > 0.0);
        // Two launches must out-score a never-launched app.
        assert!(reloaded.score("firefox") > reloaded.score("chromium"));
    }

    #[test]
    fn higher_count_outranks_lower_count() {
        let (_dir, mut store) = temp_store();
        store.record("firefox").unwrap();
        store.record("firefox").unwrap();
        store.record("chromium").unwrap();

        let ranked = store.rank(&["chromium".to_string(), "firefox".to_string()]);
        assert_eq!(ranked, vec!["firefox".to_string(), "chromium".to_string()]);
    }

    #[test]
    fn more_recent_outranks_older_same_count() {
        let (_dir, mut store) = temp_store();
        // Manually seed two records with equal count but different last_used.
        store.records.insert(
            "old".to_string(),
            UsageRecord {
                launch_count: 1,
                last_used: now_secs().saturating_sub(2 * 604_800), // ~2 weeks ago
            },
        );
        store.records.insert(
            "fresh".to_string(),
            UsageRecord {
                launch_count: 1,
                last_used: now_secs(),
            },
        );

        let ranked = store.rank(&["old".to_string(), "fresh".to_string()]);
        assert_eq!(ranked, vec!["fresh".to_string(), "old".to_string()]);
    }

    #[test]
    fn unknown_ids_rank_after_known_ids() {
        let (_dir, mut store) = temp_store();
        store.record("firefox").unwrap();

        let ranked = store.rank(&[
            "never-a".to_string(),
            "firefox".to_string(),
            "never-b".to_string(),
        ]);
        // firefox (known) first; the two unknowns keep their original order.
        assert_eq!(
            ranked,
            vec![
                "firefox".to_string(),
                "never-a".to_string(),
                "never-b".to_string()
            ]
        );
    }
}
