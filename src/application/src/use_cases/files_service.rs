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
    FileEntry, FileOpener, FileOperation, FilePreferences, FileSystemPort, FilesError, Pin,
    PinsPort, PreferencesPort, RecursiveSizer, ViewerFileInfo,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// The broadcast channel every file-explorer event is published on.
const FILES_EVENT_CHANNEL: &str = "files.event";

/// The largest edge, in pixels, an image preview is scaled to fit within.
const IMAGE_PREVIEW_MAX_DIMENSION: u32 = 512;

/// The most bytes read for a text preview of a document or code file.
const TEXT_PREVIEW_MAX_BYTES: usize = 4096;

/// The outcome of one `list_directory` filesystem call, shared by every
/// concurrent caller waiting on it.
type ListDirectoryOutcome = std::result::Result<Vec<FileEntry>, FilesError>;

/// In-flight `list` calls keyed by path. Each value is reached by every
/// concurrent caller for that path until the call settles, at which point the
/// entry is removed — see [`FilesService::list_in_flight`].
type ListDirectoryInFlight = HashMap<String, Arc<watch::Receiver<Option<ListDirectoryOutcome>>>>;

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
    /// In-flight `list` calls keyed by path: a request for a path already in
    /// flight awaits the existing call's result instead of starting a second
    /// filesystem walk. This is deliberately NOT a cache — the entry for a
    /// path is removed as soon as its call settles (success or error), so a
    /// later, independent call for the same path always re-walks the disk.
    list_in_flight: Arc<Mutex<ListDirectoryInFlight>>,
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
            list_in_flight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// List the entries of a directory. Concurrent callers for the same path
    /// share one underlying filesystem walk: the first caller starts the
    /// walk and every concurrent caller for that path awaits its result
    /// rather than starting its own. There is no persistent cache — as soon
    /// as the walk settles (success or error), its in-flight entry is
    /// removed, so a later, independent call for the same path always
    /// re-walks the disk.
    pub async fn list(&self, path: &str) -> Result<Vec<FileEntry>> {
        let receiver = {
            let mut in_flight = Self::lock_list_in_flight(&self.list_in_flight);
            in_flight
                .entry(path.to_string())
                .or_insert_with(|| {
                    let (sender, receiver) = watch::channel(None);
                    let receiver = Arc::new(receiver);
                    let worker_receiver = receiver.clone();
                    let filesystem = self.filesystem.clone();
                    let worker_in_flight = self.list_in_flight.clone();
                    let path_owned = path.to_string();
                    // Spawn before releasing the map lock or awaiting: cancelling any
                    // caller cannot cancel initialization, the walk, or its cleanup.
                    tokio::spawn(async move {
                        let outcome = filesystem.list_directory(&path_owned).await;
                        let mut in_flight = Self::lock_list_in_flight(&worker_in_flight);
                        if in_flight
                            .get(&path_owned)
                            .is_some_and(|current| Arc::ptr_eq(current, &worker_receiver))
                        {
                            in_flight.remove(&path_owned);
                        }
                        // Publish only after eviction, so completed callers can immediately
                        // request a fresh walk. No await occurs while holding the lock.
                        sender.send_replace(Some(outcome));
                    });
                    receiver
                })
                .clone()
        };

        let mut receiver = receiver.as_ref().clone();
        let outcome = receiver.wait_for(Option::is_some).await.map_err(|_| {
            FilesError::Io("directory listing worker stopped without a result".into())
        })?;
        match outcome.as_ref() {
            Some(outcome) => outcome.clone().map_err(Into::into),
            None => {
                Err(FilesError::Io("directory listing worker returned no result".into()).into())
            }
        }
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

    /// Read a file for the file viewer. Returns file type, content (for text),
    /// and metadata suitable for rendering in a preview panel.
    pub async fn read_for_viewer(&self, path: &str) -> Result<ViewerFileInfo> {
        Ok(self.filesystem.read_for_viewer(path).await?)
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

    /// Lock the `list` in-flight map, recovering the guard if a panicking
    /// task poisoned the mutex. The guard is never held across an await, so
    /// recovery is safe.
    fn lock_list_in_flight(
        in_flight: &Mutex<ListDirectoryInFlight>,
    ) -> std::sync::MutexGuard<'_, ListDirectoryInFlight> {
        in_flight
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
    use std::sync::atomic::AtomicUsize;
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
    ///
    /// `list_calls`, `entered`, `proceed`, and `gate_listing` support testing
    /// genuine overlap between concurrent `list_directory` calls. The real
    /// fake otherwise resolves instantly with no `.await` suspension point, so
    /// two instantly-resolving futures run to completion sequentially even
    /// under `tokio::join!` — that setup cannot exercise sharing of truly
    /// overlapping requests. The handshake forces real overlap: when
    /// `gate_listing` is `true`, `list_directory` increments `list_calls`,
    /// notifies `entered` (so a test knows this call has genuinely entered the
    /// fake), then awaits `proceed` before returning — nothing resolves until
    /// the test explicitly releases it.
    struct FakeFileSystem {
        entries: Vec<FileEntry>,
        drives: Vec<DriveInfo>,
        stat_entry: FileEntry,
        text_preview: String,
        image_preview: String,
        perform_error: Option<FilesError>,
        list_calls: Arc<AtomicUsize>,
        entered: Arc<tokio::sync::Notify>,
        proceed: Arc<tokio::sync::Notify>,
        gate_listing: bool,
        /// When set, `list_directory` fails with this error instead of
        /// returning `entries`. Used to verify a failed listing is shared by
        /// concurrent callers the same way a successful one is, and is not
        /// cached past settling either.
        list_error: Option<FilesError>,
        /// An artificial per-call delay standing in for a slow real walk,
        /// used only by the coalescing timing benchmark so its comparison
        /// is deterministic and independent of actual disk speed. Zero for
        /// every other test.
        list_delay: std::time::Duration,
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
                list_calls: Arc::new(AtomicUsize::new(0)),
                entered: Arc::new(tokio::sync::Notify::new()),
                proceed: Arc::new(tokio::sync::Notify::new()),
                gate_listing: false,
                list_error: None,
                list_delay: std::time::Duration::ZERO,
            }
        }
    }

    #[async_trait]
    impl FileSystemPort for FakeFileSystem {
        async fn list_directory(
            &self,
            _path: &str,
        ) -> std::result::Result<Vec<FileEntry>, FilesError> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            if self.gate_listing {
                self.proceed.notified().await;
            }
            if !self.list_delay.is_zero() {
                tokio::time::sleep(self.list_delay).await;
            }
            match &self.list_error {
                Some(error) => Err(error.clone()),
                None => Ok(self.entries.clone()),
            }
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
        async fn read_for_viewer(
            &self,
            _path: &str,
        ) -> std::result::Result<ViewerFileInfo, FilesError> {
            // Not tested in this module; a minimal stub is sufficient.
            Ok(ViewerFileInfo {
                content: String::new(),
                file_type: quantum_domain::ViewerFileType::Text,
                language: None,
                filename: "test.txt".to_string(),
                directory: "/tmp".to_string(),
                mime_type: None,
                size: 0,
                uri: None,
            })
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
    async fn concurrent_list_requests_for_same_path_share_one_filesystem_call() {
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let proceed = Arc::new(tokio::sync::Notify::new());
        let filesystem = FakeFileSystem {
            entries: vec![sample_entry("a.txt", ContentKind::Other)],
            list_calls: calls.clone(),
            entered: entered.clone(),
            proceed: proceed.clone(),
            gate_listing: true,
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
        let service = Arc::new(service);

        let first = tokio::spawn({
            let service = service.clone();
            async move { service.list("/home/user").await }
        });
        bounded(entered.notified()).await;
        let second = service.list("/home/user");
        tokio::pin!(second);
        assert!(futures::poll!(&mut second).is_pending());

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a second concurrent call must not re-enter the filesystem"
        );

        proceed.notify_waiters();
        let (first, second) = bounded(async { tokio::join!(first, second) }).await;
        assert!(first.expect("first task").is_ok());
        assert!(second.is_ok());
    }

    #[tokio::test]
    async fn sequential_list_requests_after_settling_each_hit_the_filesystem() {
        let calls = Arc::new(AtomicUsize::new(0));
        let filesystem = FakeFileSystem {
            list_calls: calls.clone(),
            gate_listing: false,
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
        service.list("/home/user").await.expect("first");
        service.list("/home/user").await.expect("second");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a later, independent call must re-walk the disk, not reuse a settled entry"
        );
    }

    async fn bounded<Output>(future: impl std::future::Future<Output = Output>) -> Output {
        tokio::time::timeout(std::time::Duration::from_secs(2), future)
            .await
            .expect("listing handshake timed out")
    }

    #[tokio::test]
    async fn aborting_list_initializer_preserves_one_walk_for_surviving_waiter() {
        for list_error in [
            None,
            Some(FilesError::PermissionDenied("/home/user".into())),
        ] {
            let filesystem = FakeFileSystem {
                entries: vec![sample_entry("a.txt", ContentKind::Other)],
                gate_listing: true,
                list_error: list_error.clone(),
                ..Default::default()
            };
            let calls = filesystem.list_calls.clone();
            let entered = filesystem.entered.clone();
            let proceed = filesystem.proceed.clone();
            let (service, _) = build_service(
                filesystem,
                FakeWatcher { changes: vec![] },
                FakeSizer { updates: vec![] },
                FakePins { pins: vec![] },
                FakeApplications {
                    applications: vec![],
                },
            );
            let service = Arc::new(service);
            let initializer = tokio::spawn({
                let service = service.clone();
                async move { service.list("/home/user").await }
            });
            bounded(entered.notified()).await;
            let waiter = service.list("/home/user");
            tokio::pin!(waiter);
            assert!(futures::poll!(&mut waiter).is_pending());
            initializer.abort();
            assert!(bounded(initializer)
                .await
                .expect_err("cancelled")
                .is_cancelled());
            // Poll after cancellation, before releasing the walk, to expose reinitialization.
            assert!(futures::poll!(&mut waiter).is_pending());
            proceed.notify_one();
            let outcome = bounded(waiter).await;
            match list_error {
                Some(error) => assert_eq!(
                    serde_json::to_value(outcome.expect_err("shared error")).unwrap(),
                    serde_json::to_value(crate::error::ApplicationError::Files(error)).unwrap(),
                ),
                None => assert_eq!(outcome.expect("shared entries").len(), 1),
            }
            assert_eq!(calls.load(Ordering::SeqCst), 1, "abort restarted the walk");
            assert!(FilesService::lock_list_in_flight(&service.list_in_flight).is_empty());
        }
    }

    #[tokio::test]
    async fn aborting_last_list_caller_still_cleans_up_success_and_failure() {
        for list_error in [None, Some(FilesError::NotFound("/home/user".into()))] {
            let filesystem = FakeFileSystem {
                gate_listing: true,
                list_error: list_error.clone(),
                ..Default::default()
            };
            let calls = filesystem.list_calls.clone();
            let entered = filesystem.entered.clone();
            let proceed = filesystem.proceed.clone();
            let (service, _) = build_service(
                filesystem,
                FakeWatcher { changes: vec![] },
                FakeSizer { updates: vec![] },
                FakePins { pins: vec![] },
                FakeApplications {
                    applications: vec![],
                },
            );
            let service = Arc::new(service);
            let caller = tokio::spawn({
                let service = service.clone();
                async move { service.list("/home/user").await }
            });
            bounded(entered.notified()).await;
            caller.abort();
            assert!(bounded(caller).await.expect_err("cancelled").is_cancelled());
            proceed.notify_one();
            bounded(async {
                loop {
                    if FilesService::lock_list_in_flight(&service.list_in_flight).is_empty() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            })
            .await;
            let next = service.list("/home/user");
            tokio::pin!(next);
            assert!(futures::poll!(&mut next).is_pending());
            bounded(entered.notified()).await;
            proceed.notify_one();
            let outcome = bounded(next).await;
            assert_eq!(outcome.is_err(), list_error.is_some());
            assert_eq!(calls.load(Ordering::SeqCst), 2, "settled result was cached");
            assert!(FilesService::lock_list_in_flight(&service.list_in_flight).is_empty());
        }
    }

    #[tokio::test]
    async fn concurrent_list_requests_for_different_paths_start_independent_walks() {
        let filesystem = FakeFileSystem {
            gate_listing: true,
            ..Default::default()
        };
        let calls = filesystem.list_calls.clone();
        let entered = filesystem.entered.clone();
        let proceed = filesystem.proceed.clone();
        let (service, _) = build_service(
            filesystem,
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );
        let first = service.list("/first");
        let second = service.list("/second");
        tokio::pin!(first, second);
        assert!(futures::poll!(&mut first).is_pending());
        bounded(entered.notified()).await;
        assert!(futures::poll!(&mut second).is_pending());
        bounded(entered.notified()).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        proceed.notify_waiters();
        assert!(bounded(first).await.is_ok());
        assert!(bounded(second).await.is_ok());
        assert!(FilesService::lock_list_in_flight(&service.list_in_flight).is_empty());
    }

    #[tokio::test]
    async fn completing_old_list_worker_does_not_remove_newer_path_generation() {
        let filesystem = FakeFileSystem {
            gate_listing: true,
            ..Default::default()
        };
        let calls = filesystem.list_calls.clone();
        let entered = filesystem.entered.clone();
        let proceed = filesystem.proceed.clone();
        let (service, _) = build_service(
            filesystem,
            FakeWatcher { changes: vec![] },
            FakeSizer { updates: vec![] },
            FakePins { pins: vec![] },
            FakeApplications {
                applications: vec![],
            },
        );
        let old = service.list("/home/user");
        tokio::pin!(old);
        assert!(futures::poll!(&mut old).is_pending());
        bounded(entered.notified()).await;
        // Force replacement to exercise the identity guard on a late completion.
        FilesService::lock_list_in_flight(&service.list_in_flight).remove("/home/user");
        let newer = service.list("/home/user");
        tokio::pin!(newer);
        assert!(futures::poll!(&mut newer).is_pending());
        bounded(entered.notified()).await;
        let newer_entry = FilesService::lock_list_in_flight(&service.list_in_flight)
            .get("/home/user")
            .expect("newer generation")
            .clone();
        // Notify releases registered waiters in order: the old walk finishes first.
        proceed.notify_one();
        assert!(bounded(old).await.is_ok());
        assert!(Arc::ptr_eq(
            FilesService::lock_list_in_flight(&service.list_in_flight)
                .get("/home/user")
                .expect("newer entry retained"),
            &newer_entry,
        ));
        proceed.notify_one();
        assert!(bounded(newer).await.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(FilesService::lock_list_in_flight(&service.list_in_flight).is_empty());
    }

    #[tokio::test]
    async fn list_error_is_shared_by_concurrent_callers_and_not_cached_after_settling() {
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let proceed = Arc::new(tokio::sync::Notify::new());
        let filesystem = FakeFileSystem {
            list_calls: calls.clone(),
            entered: entered.clone(),
            proceed: proceed.clone(),
            gate_listing: true,
            list_error: Some(FilesError::NotFound("/home/user".to_string())),
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
        let service = Arc::new(service);

        let first = tokio::spawn({
            let service = service.clone();
            async move { service.list("/home/user").await }
        });
        bounded(entered.notified()).await;
        let second = service.list("/home/user");
        tokio::pin!(second);
        assert!(futures::poll!(&mut second).is_pending());

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a failing walk must still be shared, not re-entered, while in flight"
        );

        proceed.notify_waiters();
        let (first, second) = bounded(async { tokio::join!(first, second) }).await;
        assert_eq!(
            serde_json::to_value(first.expect("first task").expect_err("first error")).unwrap(),
            serde_json::to_value(second.expect_err("second error")).unwrap(),
            "callers must receive the same typed failure"
        );

        // The failure must not be cached either: a later call re-walks. The
        // fake is still gated (as it was for every call so far in this
        // fixture), so the third walk must be released the same way as the
        // first/second pair, via `entered` then `proceed`.
        let third = tokio::spawn({
            let service = service.clone();
            async move { service.list("/home/user").await }
        });
        bounded(entered.notified()).await;
        proceed.notify_waiters();
        let third_outcome = bounded(third).await.expect("third task");
        assert!(third_outcome.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn aborting_a_waiting_caller_does_not_corrupt_the_shared_in_flight_entry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let proceed = Arc::new(tokio::sync::Notify::new());
        let filesystem = FakeFileSystem {
            entries: vec![sample_entry("a.txt", ContentKind::Other)],
            list_calls: calls.clone(),
            entered: entered.clone(),
            proceed: proceed.clone(),
            gate_listing: true,
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
        let service = Arc::new(service);

        // Start the worker before polling the second caller into its wait.
        let first = tokio::spawn({
            let service = service.clone();
            async move { service.list("/home/user").await }
        });
        bounded(entered.notified()).await;

        let mut second = Box::pin(service.list("/home/user"));
        assert!(futures::poll!(&mut second).is_pending());

        // Cancel the waiting caller while it is still waiting on the shared
        // in-flight entry — this must not disturb the still-in-progress
        // first call or leave the in-flight map in a wedged state.
        drop(second);

        proceed.notify_waiters();
        let first_outcome = bounded(first).await.expect("first task");
        assert!(
            first_outcome.is_ok(),
            "the in-progress call must complete normally despite the other waiter's abort"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the abort must not trigger a second filesystem walk"
        );

        // After settling, a later call must re-walk the disk rather than
        // finding a wedged or stale in-flight entry left over by the abort.
        // The fake is still gated, so release this walk the same way as the
        // first one, via `entered` then `proceed`.
        let third = tokio::spawn({
            let service = service.clone();
            async move { service.list("/home/user").await }
        });
        bounded(entered.notified()).await;
        proceed.notify_waiters();
        let third_outcome = bounded(third).await.expect("third task");
        assert!(third_outcome.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// Ignored timing benchmark for Task 3 of the `file-viewer-improvements`
    /// plan, at the service layer. Three overlapping calls are synthetic:
    /// the explorer awaits validation before starting its two pane loads.
    /// That actual ordering requires two backend walks, not one. Both
    /// orderings are measured here; neither measures frontend responsiveness.
    ///
    /// The underlying filesystem port here is a fake with an artificial
    /// per-call delay standing in for a slow real walk, so the comparison
    /// is deterministic and independent of actual disk speed; the real
    /// backend listing cost on a synthetic large directory is measured
    /// separately by `quantum_files::filesystem`'s own ignored benchmark.
    /// This measures ONLY the service-layer coalescing effect — it makes NO
    /// claim about real disk-listing cost or about this being the dominant
    /// cause of any user-perceived freeze; see this crate's companion
    /// benchmark and the plan's Task 3 for that live, separated
    /// measurement, which needs the real installed daemon and is not
    /// performed here.
    ///
    /// Run explicitly:
    /// `./scripts/devsh.sh cargo test -p quantum-application -- --ignored benchmark_three_concurrent_same_path_listings_pay_for_one_walk --nocapture`
    #[tokio::test]
    #[ignore = "timing benchmark, not a correctness test; run explicitly, see doc comment"]
    async fn benchmark_three_concurrent_same_path_listings_pay_for_one_walk() {
        use std::time::{Duration, Instant};

        let simulated_walk_cost = Duration::from_millis(50);
        let calls = Arc::new(AtomicUsize::new(0));
        let filesystem = FakeFileSystem {
            entries: vec![sample_entry("a.txt", ContentKind::Other)],
            list_calls: calls.clone(),
            list_delay: simulated_walk_cost,
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

        // Synthetic overlap, not the explorer's sequential validation flow.
        let started = Instant::now();
        let (validation, pane_one, pane_two) = tokio::join!(
            service.list("/home/user"),
            service.list("/home/user"),
            service.list("/home/user"),
        );
        let elapsed = started.elapsed();

        assert!(validation.is_ok() && pane_one.is_ok() && pane_two.is_ok());
        let walk_count = calls.load(Ordering::SeqCst);

        eprintln!(
            "benchmark: three concurrent same-path list() calls (\
             synthetic overlap, not the explorer's actual open ordering) \
             completed in {elapsed:?} against a simulated {simulated_walk_cost:?}-per-walk \
             cost, with {walk_count} real filesystem walk(s) performed (uncoalesced \
             would be 3). This is a service-layer coalescing measurement only; it \
             says nothing about real disk-listing cost or about the dominant cause \
             of any user-perceived freeze."
        );

        assert_eq!(
            walk_count, 1,
            "three concurrent callers for the same path must share one walk"
        );
        // Coalesced calls finish close to the cost of ONE walk, not three.
        // Generous margin to stay robust under scheduling jitter — this
        // benchmark reports a number, it does not assert a tight bound.
        assert!(
            elapsed < simulated_walk_cost * 3,
            "coalesced calls took {elapsed:?}, expected well under 3x the \
             per-walk cost {simulated_walk_cost:?}"
        );

        calls.store(0, Ordering::SeqCst);
        let started = Instant::now();
        service.list("/home/user").await.expect("validation");
        let (pane_one, pane_two) =
            tokio::join!(service.list("/home/user"), service.list("/home/user"),);
        assert!(pane_one.is_ok() && pane_two.is_ok());
        let elapsed = started.elapsed();
        let walk_count = calls.load(Ordering::SeqCst);
        assert_eq!(walk_count, 2, "validation settles before the pane loads");
        eprintln!(
            "benchmark: actual validation-then-two-overlapping-pane ordering completed \
             in {elapsed:?} with {walk_count} backend walks against simulated \
             {simulated_walk_cost:?}-per-walk cost. No disk or frontend responsiveness claim."
        );
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
            .set_preferences(FilePreferences {
                show_hidden: false,
                pinned_actions: Vec::new(),
            })
            .await
            .expect("set preferences");

        assert_eq!(
            service.get_preferences().await,
            FilePreferences {
                show_hidden: false,
                pinned_actions: Vec::new(),
            }
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
