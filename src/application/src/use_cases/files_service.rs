//! File explorer service use case.
//!
//! Core orchestration for the file-explorer feature. Injects the domain ports
//! (`FileSystemPort`, `DirectoryWatcher`, `FileOpener`, `RecursiveSizer`,
//! `PinsPort`, `ApplicationCatalog`, `EventBus`) and turns frontend requests
//! into port calls, publishing streaming updates on the `files.event` channel.
//! This crate depends only on `quantum_domain`; it never touches infrastructure
//! directly. The real ports are injected by the daemon in a later wiring task.
//!
//! Event payloads published on `files.event` are JSON objects discriminated by
//! a snake_case `event` field. They are a stable IPC contract mirrored by the
//! client DTO task:
//!
//! - `{ "event": "changed", "path": "<path>" }`
//! - `{ "event": "size", "path": "<path>", "bytes": <u64>, "complete": <bool> }`
//! - `{ "event": "operation_complete", "operation": <FileOperation> }`
//! - `{ "event": "operation_failed", "message": "<string>" }`

use crate::error::Result;
use futures::stream::StreamExt;
use quantum_domain::{
    ApplicationCatalog, ApplicationInfo, ContentKind, DirectoryWatcher, DriveInfo, EventBus,
    FileEntry, FileOpener, FileOperation, FilePreferences, FileSystemPort, Pin, PinsPort,
    PreferencesPort, RecursiveSizer,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

/// The broadcast channel every file-explorer event is published on.
const FILES_EVENT_CHANNEL: &str = "files.event";

/// The largest edge, in pixels, an image preview is scaled to fit within.
const IMAGE_PREVIEW_MAX_DIMENSION: u32 = 512;

/// The most bytes read for a text preview of a document or code file.
const TEXT_PREVIEW_MAX_BYTES: usize = 4096;

/// Which kind of preview a [`PreviewPayload`] carries. `None` means the entry
/// is not previewable and `data` is empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewKind {
    Image,
    Text,
    None,
}

/// A file preview response. `data` is a base64 data URI for an image preview or
/// UTF-8 text for a text preview, and empty when `kind` is [`PreviewKind::None`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewPayload {
    pub kind: PreviewKind,
    pub data: String,
}

/// The explorer sidebar's places: the user's pinned locations plus the
/// currently mounted drives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Places {
    pub pins: Vec<Pin>,
    pub drives: Vec<DriveInfo>,
}

/// A reference-counted subscription: the single spawned forwarding task shared
/// by every subscriber of a path, plus the number of live subscribers. The task
/// (and, for a watch, the underlying `notify` watch armed once) is torn down
/// only when the count falls back to zero. Dual-pane is the default and both
/// panes start on the same path, so subscriptions must be shared rather than
/// clobbering each other.
struct Subscription {
    count: usize,
    handle: JoinHandle<()>,
    /// Which walk armed this handle. The forwarding task removes its own handle
    /// on stream end only when the stored generation still matches the one it
    /// was spawned with, so a newer `sizes()` for the same path is never
    /// clobbered. Watch subscriptions do not self-remove and always use `0`.
    generation: u64,
}

/// Orchestrates the file-explorer subsystem. Holds the injected ports plus the
/// per-path reference-counted handles of the spawned watch and size-computation
/// tasks so they can be shared across panes and torn down on the final
/// `unwatch` / `cancel_sizes`.
pub struct FilesService {
    filesystem: Arc<dyn FileSystemPort>,
    watcher: Arc<dyn DirectoryWatcher>,
    opener: Arc<dyn FileOpener>,
    sizer: Arc<dyn RecursiveSizer>,
    pins: Arc<dyn PinsPort>,
    preferences: Arc<dyn PreferencesPort>,
    applications: Arc<dyn ApplicationCatalog>,
    event_bus: Arc<dyn EventBus>,
    /// Reference-counted per-path directory-watch subscriptions. Keyed by the
    /// watched path; the underlying watch is armed once and released only when
    /// the last subscriber calls `unwatch`.
    watch_handles: Mutex<HashMap<String, Subscription>>,
    /// Reference-counted per-path recursive-size subscriptions. Keyed by the
    /// path being sized; the computation runs once and is cancelled only when
    /// the last subscriber calls `cancel_sizes`. Shared with each forwarding
    /// task (hence `Arc`) so a completed walk can remove its own handle.
    size_handles: Arc<Mutex<HashMap<String, Subscription>>>,
    /// Monotonic source of walk generations, stamped onto each new size
    /// subscription so a completing walk only removes the handle it created.
    size_generation: AtomicU64,
}

impl FilesService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        filesystem: Arc<dyn FileSystemPort>,
        watcher: Arc<dyn DirectoryWatcher>,
        opener: Arc<dyn FileOpener>,
        sizer: Arc<dyn RecursiveSizer>,
        pins: Arc<dyn PinsPort>,
        preferences: Arc<dyn PreferencesPort>,
        applications: Arc<dyn ApplicationCatalog>,
        event_bus: Arc<dyn EventBus>,
    ) -> Self {
        Self {
            filesystem,
            watcher,
            opener,
            sizer,
            pins,
            preferences,
            applications,
            event_bus,
            watch_handles: Mutex::new(HashMap::new()),
            size_handles: Arc::new(Mutex::new(HashMap::new())),
            size_generation: AtomicU64::new(0),
        }
    }

    /// List the entries of a directory.
    pub async fn list(&self, path: &str) -> Result<Vec<FileEntry>> {
        Ok(self.filesystem.list_directory(path).await?)
    }

    /// Stat a single path.
    pub async fn stat(&self, path: &str) -> Result<FileEntry> {
        Ok(self.filesystem.stat(path).await?)
    }

    /// Search under `root` for entries matching `query`, capped at `limit`.
    pub async fn search(&self, root: &str, query: &str, limit: usize) -> Result<Vec<FileEntry>> {
        Ok(self.filesystem.search(root, query, limit).await?)
    }

    /// Produce a preview for `path`, choosing an image or text preview from the
    /// entry's content kind, or a `None` preview when it is not previewable.
    pub async fn preview(&self, path: &str) -> Result<PreviewPayload> {
        let entry = self.filesystem.stat(path).await?;
        match entry.content_kind {
            ContentKind::Image => {
                let data = self
                    .filesystem
                    .read_image_preview(path, IMAGE_PREVIEW_MAX_DIMENSION)
                    .await?;
                Ok(PreviewPayload {
                    kind: PreviewKind::Image,
                    data,
                })
            }
            ContentKind::Document | ContentKind::Code => {
                let data = self
                    .filesystem
                    .read_text_preview(path, TEXT_PREVIEW_MAX_BYTES)
                    .await?;
                Ok(PreviewPayload {
                    kind: PreviewKind::Text,
                    data,
                })
            }
            ContentKind::Archive | ContentKind::Music | ContentKind::Other => Ok(PreviewPayload {
                kind: PreviewKind::None,
                data: String::new(),
            }),
        }
    }

    /// The explorer sidebar places: pinned locations plus mounted drives.
    pub async fn places(&self) -> Result<Places> {
        let pins = self.pins.load().await;
        let drives = self.filesystem.mounts().await?;
        Ok(Places { pins, drives })
    }

    /// Add a pinned location, returning the full pin list after the change.
    pub async fn pin(&self, pin: Pin) -> Result<Vec<Pin>> {
        Ok(self.pins.add(pin).await?)
    }

    /// Remove the pin at `path`, returning the full pin list after the change.
    pub async fn unpin(&self, path: &str) -> Result<Vec<Pin>> {
        Ok(self.pins.remove(path).await?)
    }

    /// Load the persisted explorer preferences. Never fails: a missing or
    /// unreadable store yields the defaults.
    pub async fn get_preferences(&self) -> FilePreferences {
        self.preferences.load().await
    }

    /// Persist the explorer preferences, reporting an input/output failure.
    pub async fn set_preferences(&self, preferences: FilePreferences) -> Result<()> {
        self.preferences.save(preferences).await?;
        Ok(())
    }

    /// Perform a mutating filesystem operation. On success, publish an
    /// `operation_complete` event; on failure, publish an `operation_failed`
    /// event AND return the error.
    pub async fn operation(&self, operation: FileOperation) -> Result<()> {
        match self.filesystem.perform(operation.clone()).await {
            Ok(()) => {
                let payload = serde_json::json!({
                    "event": "operation_complete",
                    "operation": operation,
                });
                self.publish(&payload).await;
                Ok(())
            }
            Err(error) => {
                let payload = serde_json::json!({
                    "event": "operation_failed",
                    "message": error.to_string(),
                });
                self.publish(&payload).await;
                Err(error.into())
            }
        }
    }

    /// Open a file or directory with its default handler.
    pub async fn open(&self, path: &str) -> Result<()> {
        Ok(self.opener.open(path).await?)
    }

    /// Open a file or directory with a specific desktop application.
    pub async fn open_with(&self, path: &str, desktop_id: &str) -> Result<()> {
        Ok(self.opener.open_with(path, desktop_id).await?)
    }

    /// Open a terminal rooted at `directory`.
    pub async fn open_terminal(&self, directory: &str) -> Result<()> {
        Ok(self.opener.open_terminal(directory).await?)
    }

    /// The applications offered by the "Open with" menu.
    pub async fn applications(&self) -> Vec<ApplicationInfo> {
        self.applications.list_applications().await
    }

    /// Start watching `path`, spawning a task that republishes each change as a
    /// `changed` event on `files.event`. Reference-counted per path: a second
    /// subscriber (for example the other pane, which starts on the same path)
    /// increments the count and shares the single armed watch and forwarding
    /// task rather than arming a second `notify` watch or spawning a duplicate
    /// forwarder.
    pub fn watch(&self, path: &str) -> Result<()> {
        let mut handles = Self::lock(&self.watch_handles);
        if let Some(subscription) = handles.get_mut(path) {
            subscription.count += 1;
            return Ok(());
        }
        let stream = self.watcher.watch(path)?;
        let event_bus = self.event_bus.clone();
        let handle = tokio::spawn(async move {
            let mut stream = stream;
            while let Some(changed) = stream.next().await {
                let payload = serde_json::json!({
                    "event": "changed",
                    "path": changed,
                });
                let _ = event_bus
                    .publish(FILES_EVENT_CHANNEL, &payload.to_string())
                    .await;
            }
        });
        handles.insert(
            path.to_string(),
            Subscription {
                count: 1,
                handle,
                generation: 0,
            },
        );
        Ok(())
    }

    /// Stop watching `path`. Decrements the reference count; only when the last
    /// subscriber leaves does it abort the forwarding task and release the
    /// underlying watch, so one pane navigating away never tears down a watch
    /// the other pane still needs.
    pub fn unwatch(&self, path: &str) {
        let mut handles = Self::lock(&self.watch_handles);
        let remaining = match handles.get_mut(path) {
            Some(subscription) => {
                subscription.count -= 1;
                subscription.count
            }
            None => return,
        };
        if remaining > 0 {
            return;
        }
        if let Some(subscription) = handles.remove(path) {
            subscription.handle.abort();
        }
        drop(handles);
        self.watcher.unwatch(path);
    }

    /// Start computing the recursive size of `path`, spawning a task that
    /// republishes each update as a `size` event on `files.event`. Reference-
    /// counted per path: a second subscriber increments the count and shares the
    /// single running computation and forwarding task rather than starting a
    /// duplicate.
    pub fn sizes(&self, path: &str) {
        let mut handles = Self::lock(&self.size_handles);
        if let Some(subscription) = handles.get_mut(path) {
            subscription.count += 1;
            return;
        }
        let stream = self.sizer.compute(path);
        let event_bus = self.event_bus.clone();
        let generation = self.size_generation.fetch_add(1, Ordering::Relaxed);
        let task_handles = self.size_handles.clone();
        let path_owned = path.to_string();
        let handle = tokio::spawn(async move {
            let mut stream = stream;
            while let Some(update) = stream.next().await {
                let payload = serde_json::json!({
                    "event": "size",
                    "path": update.path,
                    "bytes": update.bytes,
                    "complete": update.complete,
                });
                let _ = event_bus
                    .publish(FILES_EVENT_CHANNEL, &payload.to_string())
                    .await;
            }
            // The walk finished (or the consumer dropped). Remove our own handle
            // so a later `sizes()` for this path starts a fresh walk instead of
            // finding a dead handle. Guard by generation so we never remove a
            // newer walk's handle. No await inside, so holding the lock is safe.
            if let Ok(mut map) = task_handles.lock() {
                if map
                    .get(&path_owned)
                    .is_some_and(|subscription| subscription.generation == generation)
                {
                    map.remove(&path_owned);
                }
            }
        });
        handles.insert(
            path.to_string(),
            Subscription {
                count: 1,
                handle,
                generation,
            },
        );
    }

    /// Cancel the recursive-size computation for `path`. Decrements the
    /// reference count; only when the last subscriber leaves does it abort the
    /// forwarding task and cancel the underlying computation, so one pane never
    /// tears down a size stream the other pane still needs.
    pub fn cancel_sizes(&self, path: &str) {
        let mut handles = Self::lock(&self.size_handles);
        let remaining = match handles.get_mut(path) {
            Some(subscription) => {
                subscription.count -= 1;
                subscription.count
            }
            None => return,
        };
        if remaining > 0 {
            return;
        }
        if let Some(subscription) = handles.remove(path) {
            subscription.handle.abort();
        }
        drop(handles);
        self.sizer.cancel(path);
    }

    /// Publish a JSON event payload on the `files.event` channel, ignoring a
    /// transient publish failure so a dropped subscriber cannot wedge the
    /// caller.
    async fn publish(&self, payload: &serde_json::Value) {
        let _ = self
            .event_bus
            .publish(FILES_EVENT_CHANNEL, &payload.to_string())
            .await;
    }

    /// Lock a handle map, recovering the guard if a panicking task poisoned the
    /// mutex. The guard is never held across an await, so recovery is safe.
    fn lock(
        handles: &Mutex<HashMap<String, Subscription>>,
    ) -> std::sync::MutexGuard<'_, HashMap<String, Subscription>> {
        handles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};
    use quantum_domain::{FileEntryKind, FilesError, PermissionClass, SizeUpdate};
    use tokio::sync::Mutex as TokioMutex;

    fn sample_entry(name: &str, content_kind: ContentKind) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: format!("/home/user/{name}"),
            kind: FileEntryKind::File,
            size: 10,
            recursive_size: None,
            modified_epoch_seconds: 0,
            owner: "user".to_string(),
            permissions: "rw-r--r--".to_string(),
            permission_class: PermissionClass::Normal,
            symlink_target: None,
            content_kind,
        }
    }

    /// Configurable filesystem mock. Each field seeds a fixed answer; `perform`
    /// fails with `perform_error` when set and otherwise succeeds.
    struct FakeFileSystem {
        entries: Vec<FileEntry>,
        drives: Vec<DriveInfo>,
        stat_entry: FileEntry,
        text_preview: String,
        image_preview: String,
        perform_error: Option<FilesError>,
    }

    impl Default for FakeFileSystem {
        fn default() -> Self {
            Self {
                entries: Vec::new(),
                drives: Vec::new(),
                stat_entry: sample_entry("stat.txt", ContentKind::Other),
                text_preview: String::new(),
                image_preview: String::new(),
                perform_error: None,
            }
        }
    }

    #[async_trait]
    impl FileSystemPort for FakeFileSystem {
        async fn list_directory(
            &self,
            _path: &str,
        ) -> std::result::Result<Vec<FileEntry>, FilesError> {
            Ok(self.entries.clone())
        }
        async fn stat(&self, _path: &str) -> std::result::Result<FileEntry, FilesError> {
            Ok(self.stat_entry.clone())
        }
        async fn mounts(&self) -> std::result::Result<Vec<DriveInfo>, FilesError> {
            Ok(self.drives.clone())
        }
        async fn read_text_preview(
            &self,
            _path: &str,
            _max_bytes: usize,
        ) -> std::result::Result<String, FilesError> {
            Ok(self.text_preview.clone())
        }
        async fn read_image_preview(
            &self,
            _path: &str,
            _max_dimension: u32,
        ) -> std::result::Result<String, FilesError> {
            Ok(self.image_preview.clone())
        }
        async fn perform(&self, _operation: FileOperation) -> std::result::Result<(), FilesError> {
            match &self.perform_error {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }
        async fn search(
            &self,
            _root: &str,
            _query: &str,
            _limit: usize,
        ) -> std::result::Result<Vec<FileEntry>, FilesError> {
            Ok(self.entries.clone())
        }
    }

    /// Watcher mock whose `watch` yields the paths in `changes` once.
    struct FakeWatcher {
        changes: Vec<String>,
    }

    impl DirectoryWatcher for FakeWatcher {
        fn watch(
            &self,
            _path: &str,
        ) -> std::result::Result<BoxStream<'static, String>, FilesError> {
            Ok(stream::iter(self.changes.clone()).boxed())
        }
        fn unwatch(&self, _path: &str) {}
    }

    /// Opener mock: records nothing, always succeeds.
    struct FakeOpener;

    #[async_trait]
    impl FileOpener for FakeOpener {
        async fn open(&self, _path: &str) -> std::result::Result<(), FilesError> {
            Ok(())
        }
        async fn open_with(
            &self,
            _path: &str,
            _desktop_id: &str,
        ) -> std::result::Result<(), FilesError> {
            Ok(())
        }
        async fn open_terminal(&self, _directory: &str) -> std::result::Result<(), FilesError> {
            Ok(())
        }
    }

    /// Sizer mock whose `compute` yields the supplied updates once.
    struct FakeSizer {
        updates: Vec<SizeUpdate>,
    }

    impl RecursiveSizer for FakeSizer {
        fn compute(&self, _path: &str) -> BoxStream<'static, SizeUpdate> {
            stream::iter(self.updates.clone()).boxed()
        }
        fn cancel(&self, _path: &str) {}
    }

    /// Pins mock seeded with a fixed list; mutations are not exercised here.
    struct FakePins {
        pins: Vec<Pin>,
    }

    #[async_trait]
    impl PinsPort for FakePins {
        async fn load(&self) -> Vec<Pin> {
            self.pins.clone()
        }
        async fn add(&self, pin: Pin) -> std::result::Result<Vec<Pin>, FilesError> {
            let mut pins = self.pins.clone();
            pins.push(pin);
            Ok(pins)
        }
        async fn remove(&self, path: &str) -> std::result::Result<Vec<Pin>, FilesError> {
            Ok(self
                .pins
                .iter()
                .filter(|pin| pin.path != path)
                .cloned()
                .collect())
        }
    }

    /// Preferences mock backed by an in-memory cell so `save` then `load`
    /// round-trips within a test. Starts at the defaults.
    struct FakePreferences {
        stored: Mutex<FilePreferences>,
    }

    impl FakePreferences {
        fn new() -> Self {
            Self {
                stored: Mutex::new(FilePreferences::default()),
            }
        }
    }

    #[async_trait]
    impl PreferencesPort for FakePreferences {
        async fn load(&self) -> FilePreferences {
            self.stored
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
        async fn save(&self, preferences: FilePreferences) -> std::result::Result<(), FilesError> {
            *self
                .stored
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = preferences;
            Ok(())
        }
    }

    /// Application-catalog mock seeded with a fixed list.
    struct FakeApplications {
        applications: Vec<ApplicationInfo>,
    }

    #[async_trait]
    impl ApplicationCatalog for FakeApplications {
        async fn list_applications(&self) -> Vec<ApplicationInfo> {
            self.applications.clone()
        }
    }

    /// Event-bus mock that captures every `(channel, payload)` it is asked to
    /// publish, behind an async mutex so spawned tasks can record concurrently.
    struct FakeEventBus {
        events: TokioMutex<Vec<(String, String)>>,
    }

    impl FakeEventBus {
        fn new() -> Self {
            Self {
                events: TokioMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl EventBus for FakeEventBus {
        async fn publish(
            &self,
            event: &str,
            payload: &str,
        ) -> std::result::Result<(), quantum_domain::DomainError> {
            self.events
                .lock()
                .await
                .push((event.to_string(), payload.to_string()));
            Ok(())
        }
    }

    /// The captured fakes a test may inspect after driving the service. Only
    /// the event bus is asserted on; the other ports are verified through the
    /// service's return values.
    struct Fakes {
        event_bus: Arc<FakeEventBus>,
    }

    /// Assemble a `FilesService` over the supplied fakes, filling watcher and
    /// sizer with the given streams.
    fn build_service(
        filesystem: FakeFileSystem,
        watcher: FakeWatcher,
        sizer: FakeSizer,
        pins: FakePins,
        applications: FakeApplications,
    ) -> (FilesService, Fakes) {
        let event_bus = Arc::new(FakeEventBus::new());
        let service = FilesService::new(
            Arc::new(filesystem),
            Arc::new(watcher),
            Arc::new(FakeOpener),
            Arc::new(sizer),
            Arc::new(pins),
            Arc::new(FakePreferences::new()),
            Arc::new(applications),
            event_bus.clone(),
        );
        (service, Fakes { event_bus })
    }

    #[tokio::test]
    async fn list_delegates_to_filesystem() {
        let entries = vec![
            sample_entry("a.txt", ContentKind::Document),
            sample_entry("b.rs", ContentKind::Code),
        ];
        let filesystem = FakeFileSystem {
            entries: entries.clone(),
            ..Default::default()
        };
        let (service, _fakes) = build_service(
            filesystem,
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );

        let listed = service.list("/home/user").await.expect("list");
        assert_eq!(listed, entries);
    }

    #[tokio::test]
    async fn operation_success_publishes_operation_complete() {
        let (service, fakes) = build_service(
            FakeFileSystem::default(),
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );

        service
            .operation(FileOperation::NewFolder {
                parent: "/home/user".to_string(),
                name: "projects".to_string(),
            })
            .await
            .expect("operation");

        let events = fakes.event_bus.events.lock().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "files.event");
        assert!(events[0].1.contains("\"event\":\"operation_complete\""));
        assert!(events[0].1.contains("\"kind\":\"new_folder\""));
    }

    #[tokio::test]
    async fn operation_failure_publishes_operation_failed_and_returns_error() {
        let filesystem = FakeFileSystem {
            perform_error: Some(FilesError::PermissionDenied("/root".to_string())),
            ..Default::default()
        };
        let (service, fakes) = build_service(
            filesystem,
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );

        let result = service
            .operation(FileOperation::Delete {
                paths: vec!["/root/secret".to_string()],
            })
            .await;
        assert!(result.is_err());

        let events = fakes.event_bus.events.lock().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "files.event");
        assert!(events[0].1.contains("\"event\":\"operation_failed\""));
        assert!(events[0].1.contains("permission denied"));
    }

    #[tokio::test]
    async fn watch_republishes_changes() {
        let (service, fakes) = build_service(
            FakeFileSystem::default(),
            FakeWatcher {
                changes: vec!["/home/user/new.txt".to_string()],
            },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );

        service.watch("/home/user").expect("watch");
        // Let the spawned forwarding task drain the one-element stream.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let events = fakes.event_bus.events.lock().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "files.event");
        assert!(events[0].1.contains("\"event\":\"changed\""));
        assert!(events[0].1.contains("/home/user/new.txt"));
    }

    #[tokio::test]
    async fn places_merges_pins_and_drives() {
        let pins = vec![Pin {
            label: "Projects".to_string(),
            path: "/home/user/projects".to_string(),
        }];
        let drives = vec![DriveInfo {
            label: "root".to_string(),
            mount_point: "/".to_string(),
            total_bytes: 100,
            free_bytes: 40,
        }];
        let filesystem = FakeFileSystem {
            drives: drives.clone(),
            ..Default::default()
        };
        let (service, _fakes) = build_service(
            filesystem,
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: pins.clone() },
            FakeApplications {
                applications: vec![],
            },
        );

        let places = service.places().await.expect("places");
        assert_eq!(places.pins, pins);
        assert_eq!(places.drives, drives);
    }

    #[tokio::test]
    async fn preferences_set_then_get_roundtrips() {
        let (service, _fakes) = build_service(
            FakeFileSystem::default(),
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );

        // The store starts at the defaults (dotfiles shown).
        assert!(service.get_preferences().await.show_hidden);

        service
            .set_preferences(FilePreferences { show_hidden: false })
            .await
            .expect("set preferences");

        assert_eq!(
            service.get_preferences().await,
            FilePreferences { show_hidden: false }
        );
    }

    #[tokio::test]
    async fn preview_image_reads_image_preview() {
        let filesystem = FakeFileSystem {
            stat_entry: sample_entry("photo.png", ContentKind::Image),
            image_preview: "data:image/png;base64,AAAA".to_string(),
            ..Default::default()
        };
        let (service, _fakes) = build_service(
            filesystem,
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );

        let preview = service
            .preview("/home/user/photo.png")
            .await
            .expect("preview");
        assert_eq!(preview.kind, PreviewKind::Image);
        assert_eq!(preview.data, "data:image/png;base64,AAAA");
    }

    #[tokio::test]
    async fn preview_code_reads_text_preview() {
        let filesystem = FakeFileSystem {
            stat_entry: sample_entry("main.rs", ContentKind::Code),
            text_preview: "fn main() {}".to_string(),
            ..Default::default()
        };
        let (service, _fakes) = build_service(
            filesystem,
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );

        let preview = service
            .preview("/home/user/main.rs")
            .await
            .expect("preview");
        assert_eq!(preview.kind, PreviewKind::Text);
        assert_eq!(preview.data, "fn main() {}");
    }

    #[tokio::test]
    async fn preview_other_is_none() {
        let filesystem = FakeFileSystem {
            stat_entry: sample_entry("mystery.bin", ContentKind::Other),
            ..Default::default()
        };
        let (service, _fakes) = build_service(
            filesystem,
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );

        let preview = service
            .preview("/home/user/mystery.bin")
            .await
            .expect("preview");
        assert_eq!(preview.kind, PreviewKind::None);
        assert!(preview.data.is_empty());
    }

    #[tokio::test]
    async fn sizes_republishes_updates() {
        let (service, fakes) = build_service(
            FakeFileSystem::default(),
            FakeWatcher { changes: vec![] },
            FakeSizer {
                updates: vec![SizeUpdate {
                    path: "/home/user/projects".to_string(),
                    bytes: 2048,
                    complete: true,
                }],
            },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );

        service.sizes("/home/user/projects");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let events = fakes.event_bus.events.lock().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "files.event");
        assert!(events[0].1.contains("\"event\":\"size\""));
        assert!(events[0].1.contains("\"bytes\":2048"));
        assert!(events[0].1.contains("\"complete\":true"));
    }

    /// Directory-watcher mock that counts how many times the underlying watch is
    /// armed and released, and exposes a channel so a test can push change events
    /// into the live subscription's stream at will.
    struct CountingWatcher {
        watch_calls: Arc<std::sync::atomic::AtomicUsize>,
        unwatch_calls: Arc<std::sync::atomic::AtomicUsize>,
        sender: Arc<Mutex<Option<futures::channel::mpsc::UnboundedSender<String>>>>,
    }

    impl DirectoryWatcher for CountingWatcher {
        fn watch(
            &self,
            _path: &str,
        ) -> std::result::Result<BoxStream<'static, String>, FilesError> {
            self.watch_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (sender, receiver) = futures::channel::mpsc::unbounded();
            *self
                .sender
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sender);
            Ok(receiver.boxed())
        }
        fn unwatch(&self, _path: &str) {
            self.unwatch_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Recursive-sizer mock that counts computations started and cancelled, and
    /// exposes a channel so a test can push size updates into the live stream.
    struct CountingSizer {
        compute_calls: Arc<std::sync::atomic::AtomicUsize>,
        cancel_calls: Arc<std::sync::atomic::AtomicUsize>,
        sender: Arc<Mutex<Option<futures::channel::mpsc::UnboundedSender<SizeUpdate>>>>,
    }

    impl RecursiveSizer for CountingSizer {
        fn compute(&self, _path: &str) -> BoxStream<'static, SizeUpdate> {
            self.compute_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (sender, receiver) = futures::channel::mpsc::unbounded();
            *self
                .sender
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sender);
            receiver.boxed()
        }
        fn cancel(&self, _path: &str) {
            self.cancel_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn watch_is_reference_counted_across_panes() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let watch_calls = Arc::new(AtomicUsize::new(0));
        let unwatch_calls = Arc::new(AtomicUsize::new(0));
        let sender_slot = Arc::new(Mutex::new(None));
        let watcher = CountingWatcher {
            watch_calls: watch_calls.clone(),
            unwatch_calls: unwatch_calls.clone(),
            sender: sender_slot.clone(),
        };
        let event_bus = Arc::new(FakeEventBus::new());
        let service = FilesService::new(
            Arc::new(FakeFileSystem::default()),
            Arc::new(watcher),
            Arc::new(FakeOpener),
            Arc::new(FakeSizer { updates: vec![] }),
            Arc::new(FakePins { pins: vec![] }),
            Arc::new(FakePreferences::new()),
            Arc::new(FakeApplications {
                applications: vec![],
            }),
            event_bus.clone(),
        );

        // Both panes subscribe to the same path; the watcher is armed once.
        service.watch("/home/user").expect("first watch");
        service.watch("/home/user").expect("second watch");
        assert_eq!(watch_calls.load(Ordering::SeqCst), 1);

        // One pane navigates away. The subscription must stay live for the other.
        service.unwatch("/home/user");
        assert_eq!(unwatch_calls.load(Ordering::SeqCst), 0);

        let send_change = |name: &str| {
            sender_slot
                .lock()
                .unwrap()
                .as_ref()
                .expect("watcher armed")
                .unbounded_send(name.to_string())
                .expect("send change");
        };
        send_change("/home/user/still-live.txt");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        {
            let events = event_bus.events.lock().await;
            assert_eq!(events.len(), 1, "change should still forward: {events:?}");
            assert!(events[0].1.contains("/home/user/still-live.txt"));
        }

        // The second pane navigates away. Now the watch is finally released.
        service.unwatch("/home/user");
        assert_eq!(unwatch_calls.load(Ordering::SeqCst), 1);

        // With the forwarder aborted, further changes are not forwarded.
        send_change("/home/user/after-teardown.txt");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let events = event_bus.events.lock().await;
        assert_eq!(events.len(), 1, "no forwarding after teardown: {events:?}");
    }

    #[tokio::test]
    async fn sizes_is_reference_counted_across_panes() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let compute_calls = Arc::new(AtomicUsize::new(0));
        let cancel_calls = Arc::new(AtomicUsize::new(0));
        let sender_slot = Arc::new(Mutex::new(None));
        let sizer = CountingSizer {
            compute_calls: compute_calls.clone(),
            cancel_calls: cancel_calls.clone(),
            sender: sender_slot.clone(),
        };
        let event_bus = Arc::new(FakeEventBus::new());
        let service = FilesService::new(
            Arc::new(FakeFileSystem::default()),
            Arc::new(FakeWatcher { changes: vec![] }),
            Arc::new(FakeOpener),
            Arc::new(sizer),
            Arc::new(FakePins { pins: vec![] }),
            Arc::new(FakePreferences::new()),
            Arc::new(FakeApplications {
                applications: vec![],
            }),
            event_bus.clone(),
        );

        // Both panes request the size of the same path; compute runs once.
        service.sizes("/home/user/projects");
        service.sizes("/home/user/projects");
        assert_eq!(compute_calls.load(Ordering::SeqCst), 1);

        // One pane cancels. The computation must stay live for the other.
        service.cancel_sizes("/home/user/projects");
        assert_eq!(cancel_calls.load(Ordering::SeqCst), 0);

        let send_update = |bytes: u64, complete: bool| {
            sender_slot
                .lock()
                .unwrap()
                .as_ref()
                .expect("compute started")
                .unbounded_send(SizeUpdate {
                    path: "/home/user/projects".to_string(),
                    bytes,
                    complete,
                })
                .expect("send update");
        };
        send_update(1024, false);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        {
            let events = event_bus.events.lock().await;
            assert_eq!(events.len(), 1, "size should still forward: {events:?}");
            assert!(events[0].1.contains("\"bytes\":1024"));
        }

        // The second pane cancels. Now the computation is finally cancelled.
        service.cancel_sizes("/home/user/projects");
        assert_eq!(cancel_calls.load(Ordering::SeqCst), 1);

        // With the forwarder aborted, further updates are not forwarded.
        send_update(2048, true);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let events = event_bus.events.lock().await;
        assert_eq!(events.len(), 1, "no forwarding after cancel: {events:?}");
    }

    #[tokio::test]
    async fn sizes_restarts_walk_after_completion() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let compute_calls = Arc::new(AtomicUsize::new(0));
        let cancel_calls = Arc::new(AtomicUsize::new(0));
        let sender_slot = Arc::new(Mutex::new(None));
        let sizer = CountingSizer {
            compute_calls: compute_calls.clone(),
            cancel_calls: cancel_calls.clone(),
            sender: sender_slot.clone(),
        };
        let event_bus = Arc::new(FakeEventBus::new());
        let service = FilesService::new(
            Arc::new(FakeFileSystem::default()),
            Arc::new(FakeWatcher { changes: vec![] }),
            Arc::new(FakeOpener),
            Arc::new(sizer),
            Arc::new(FakePins { pins: vec![] }),
            Arc::new(FakePreferences::new()),
            Arc::new(FakeApplications {
                applications: vec![],
            }),
            event_bus.clone(),
        );

        // First walk starts and computes once.
        service.sizes("/dir");
        assert_eq!(compute_calls.load(Ordering::SeqCst), 1);

        // Forward one final update, then drop the sender so the receiver stream
        // ends. An unbounded receiver stream completes once every sender drops,
        // which drives the forwarding task's loop to exit and run its post-loop
        // handle removal.
        sender_slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .expect("compute started")
            .unbounded_send(SizeUpdate {
                path: "/dir/child".to_string(),
                bytes: 100,
                complete: true,
            })
            .expect("send update");
        *sender_slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // The completed walk removed its own handle, so a fresh request must
        // start a brand-new walk rather than sharing the dead one. Without the
        // fix this stays at 1 and folder sizes are stuck.
        service.sizes("/dir");
        assert_eq!(compute_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn applications_delegates_to_catalog() {
        let applications = vec![ApplicationInfo {
            id: "org.gnome.gedit.desktop".to_string(),
            name: "Text Editor".to_string(),
        }];
        let (service, _fakes) = build_service(
            FakeFileSystem::default(),
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: applications.clone(),
            },
        );

        assert_eq!(service.applications().await, applications);
    }

    #[tokio::test]
    async fn pin_then_unpin_round_trips_through_port() {
        let (service, _fakes) = build_service(
            FakeFileSystem::default(),
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins {
                pins: vec![Pin {
                    label: "Home".to_string(),
                    path: "/home/user".to_string(),
                }],
            },
            FakeApplications {
                applications: vec![],
            },
        );

        let after_pin = service
            .pin(Pin {
                label: "Projects".to_string(),
                path: "/home/user/projects".to_string(),
            })
            .await
            .expect("pin");
        assert_eq!(after_pin.len(), 2);

        let after_unpin = service.unpin("/home/user").await.expect("unpin");
        assert!(after_unpin.iter().all(|pin| pin.path != "/home/user"));
    }
}
