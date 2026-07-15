use crate::cursor::CursorPosition;
use crate::files::{ApplicationInfo, FileOperation, FilePreferences, FilesError, Pin};
use crate::processes::{KillSignal, ProcessSnapshot, ProcessesError};
use crate::timer::{CivilNow, Timer, TimerError, TimerStoreData};
use crate::{Action, DomainError, DriveInfo, FileEntry, Match, ProviderId, Query};
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Outcome of invoking an action.
#[derive(Debug, Clone)]
pub struct ActionOutcome {
    pub message: Option<String>,
}

/// A rectangular region, in surface-local pixels, used to describe the
/// pointer input region of a layer-shell surface. The bar's full-height
/// surface clips its input region to the visible strip (plus, while a menu
/// is open, the menu rectangle) so the otherwise-transparent area below the
/// bar does not capture screen-wide clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WindowInputRegion {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// A provider source that can search and invoke actions.
#[async_trait]
pub trait ProviderSource: Send + Sync {
    fn id(&self) -> &ProviderId;
    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError>;
    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError>;
    /// Optional event stream. Providers that publish state updates return a
    /// boxed stream of serialized events here. Default returns `None`,
    /// signalling the provider does not expose subscriptions.
    ///
    /// Events are opaque `serde_json::Value`s — each provider serializes its
    /// own state struct and the dispatcher forwards the JSON to subscribers.
    fn subscribe(&self) -> Option<futures::stream::BoxStream<'static, serde_json::Value>> {
        None
    }
    /// The provider's current state for one-shot `provider.query`, distinct
    /// from the streaming `subscribe()`. Providers that can report their state
    /// synchronously override this to return `Some(value)`; the default returns
    /// `None`, signalling the caller to fall back to taking the first emission
    /// of `subscribe()`. When overridden, the returned value MUST match the
    /// shape of `subscribe()`'s first emission so a `provider.query` answered
    /// via this explicit path is indistinguishable from one answered via the
    /// stream.
    async fn snapshot(&self) -> Option<serde_json::Value> {
        None
    }
}

/// Registry for looking up providers.
#[async_trait]
pub trait ProviderRegistry: Send + Sync {
    async fn list(&self) -> Vec<ProviderId>;
    async fn get(&self, id: &ProviderId) -> Option<Arc<dyn ProviderSource>>;
}

/// Theme storage and resolution.
#[async_trait]
pub trait ThemeStore: Send + Sync {
    async fn load_theme(&self, name: &str) -> Result<(), DomainError>;
    async fn reload(&self) -> Result<(), DomainError>;
    /// Get a file from a theme by name and relative path. Returns None if not found.
    /// This is a synchronous method since URI handlers run on the GTK thread.
    fn get_file(&self, theme_name: &str, path: &str) -> Option<Vec<u8>>;
    /// Get an asset file from the active theme. Returns None if not found.
    /// This is a synchronous method since URI handlers run on the GTK thread.
    fn get_asset(&self, path: &str) -> Option<Vec<u8>>;
    /// Get a file from a user-authored plugin's folder. Returns None if
    /// not found or if `path` would escape the plugin's directory.
    /// Implementations that don't serve plugins must explicitly return None.
    fn get_plugin_file(&self, plugin_name: &str, path: &str) -> Option<Vec<u8>>;
    /// Get resolved tokens for CSS variable injection.
    /// This is a synchronous method for use in URI handlers on the GTK thread.
    fn resolved_tokens(&self) -> std::collections::HashMap<String, String>;
}

/// Event bus for domain events.
#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(&self, event: &str, payload: &str) -> Result<(), DomainError>;
}

/// Shell output from command execution.
#[derive(Debug, Clone)]
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

/// Shell execution.
#[async_trait]
pub trait ShellExecutor: Send + Sync {
    async fn run_with_timeout(
        &self,
        command: &[String],
        timeout_ms: u64,
    ) -> Result<ShellOutput, DomainError>;
    async fn spawn_detached(&self, command: &[String]) -> Result<(), DomainError>;
}

/// Hyprland IPC client.
#[async_trait]
pub trait HyprlandClient: Send + Sync {
    async fn command(&self, cmd: &str) -> Result<String, DomainError>;
}

/// Source of user-authored plugins. Implementations walk a filesystem
/// directory (or a synthetic source in tests) and report how many
/// plugins were discovered. v1 only returns the count; richer detail
/// can be added later if `plugin.reload` consumers need it.
#[async_trait]
pub trait PluginCatalog: Send + Sync {
    async fn discover(&self) -> Result<usize, DomainError>;
}

/// Window host for managing windows.
#[async_trait]
pub trait WindowHost: Send + Sync {
    async fn open(&self, view: &str, mode: crate::WindowMode) -> Result<(), DomainError>;

    /// Resize an already-open window to the given pixel height. Used by
    /// the bar to grow its surface when a popover opens so the popover
    /// has room to render below the visible bar row, then shrink back
    /// when the popover closes. The exclusive zone (the area apps must
    /// avoid) is independent of this height and remains constant.
    ///
    /// No default impl: a forgotten override would silently clip
    /// popovers. Implementations that don't resize must explicitly
    /// return Ok(()).
    async fn set_view_height(&self, view: &str, height: u32) -> Result<(), DomainError>;

    /// Set the pointer input region of an already-open window. `Some(region)`
    /// clips pointer input to the union of the bar's visible strip and the
    /// supplied rectangle (the open menu); `None` resets the region to the
    /// strip-only default. Used by the bar so its full-height surface only
    /// captures clicks over the visible row and any open dropdown.
    ///
    /// No default impl: a forgotten override would leave a full-height
    /// surface capturing screen-wide clicks. Implementations that don't
    /// manage input regions must explicitly return Ok(()).
    async fn set_view_input_region(
        &self,
        view: &str,
        region: Option<WindowInputRegion>,
    ) -> Result<(), DomainError>;
}

/// A source of wall-clock time. Synchronous: callers need the current instant
/// without yielding. `now_unix` is seconds since the Unix epoch; `local_civil`
/// projects "now" onto the local calendar for recurring-timer arithmetic.
pub trait Clock: Send + Sync {
    fn now_unix(&self) -> u64;
    fn local_civil(&self) -> CivilNow;
}

/// Persistence for the timer subsystem's full state.
#[async_trait]
pub trait TimerStore: Send + Sync {
    async fn load(&self) -> Result<TimerStoreData, TimerError>;
    async fn save(&self, data: &TimerStoreData) -> Result<(), TimerError>;
}

/// Delivers a user-facing notification when a timer completes.
#[async_trait]
pub trait TimerNotifier: Send + Sync {
    async fn notify_complete(&self, timer: &Timer);
}

/// Broadcasts the current timer state to subscribers (for example, frontends).
pub trait TimerBroadcast: Send + Sync {
    fn publish(&self, data: &TimerStoreData);
}

/// Emits a simple user-facing notification with a short summary and a longer
/// body. Used by the launcher to surface the outcome of a command it ran
/// without routing through the full notifications provider.
#[async_trait]
pub trait NotificationEmitter: Send + Sync {
    async fn emit(&self, summary: &str, body: &str);
}

/// A progress report from a [`RecursiveSizer`] computing the total size of a
/// directory tree. Emitted repeatedly as the walk accumulates bytes; the final
/// emission for a given `path` sets `complete` to `true`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SizeUpdate {
    pub path: String,
    pub bytes: u64,
    pub complete: bool,
}

/// Reads and mutates the filesystem on behalf of the explorer. All methods take
/// borrowed paths and return typed [`FilesError`]s so no host error type leaks
/// across the IPC boundary.
#[async_trait]
pub trait FileSystemPort: Send + Sync {
    async fn list_directory(&self, path: &str) -> Result<Vec<FileEntry>, FilesError>;
    async fn stat(&self, path: &str) -> Result<FileEntry, FilesError>;
    async fn mounts(&self) -> Result<Vec<DriveInfo>, FilesError>;
    async fn read_text_preview(&self, path: &str, max_bytes: usize) -> Result<String, FilesError>;
    async fn read_image_preview(
        &self,
        path: &str,
        max_dimension: u32,
    ) -> Result<String, FilesError>;
    async fn perform(&self, operation: FileOperation) -> Result<(), FilesError>;
    async fn search(
        &self,
        root: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FileEntry>, FilesError>;
}

/// Watches a directory for changes and streams a marker string per change so
/// the explorer can refresh the affected listing. Synchronous: registering a
/// watch does not yield.
pub trait DirectoryWatcher: Send + Sync {
    fn watch(&self, path: &str) -> Result<BoxStream<'static, String>, FilesError>;
    fn unwatch(&self, path: &str);
}

/// Opens files and directories through the desktop's launch mechanisms.
#[async_trait]
pub trait FileOpener: Send + Sync {
    async fn open(&self, path: &str) -> Result<(), FilesError>;
    async fn open_with(&self, path: &str, desktop_id: &str) -> Result<(), FilesError>;
    async fn open_terminal(&self, directory: &str) -> Result<(), FilesError>;
}

/// Computes recursive on-disk sizes for the child directories of a directory,
/// streaming progress as it walks. Synchronous: starting a computation returns a
/// stream immediately.
///
/// `compute(dir)` sizes each immediate child directory of `dir`: for every entry
/// of `dir` that is itself a directory (never a symlink), it recursively sums the
/// sizes of every regular file beneath that child and emits [`SizeUpdate`]s keyed
/// by the CHILD directory's path — throttled progress updates followed by one
/// final `complete: true` per child. Regular files directly in `dir` and
/// symlinked children are not emitted, and symlinks encountered inside a child
/// are skipped rather than followed. `cancel(dir)` stops all in-flight child
/// walks promptly; a cancelled walk emits no completion item.
pub trait RecursiveSizer: Send + Sync {
    fn compute(&self, path: &str) -> BoxStream<'static, SizeUpdate>;
    fn cancel(&self, path: &str);
}

/// Persistence for the explorer's user-pinned sidebar locations. Each mutating
/// method returns the full pin list after the change so a caller can broadcast
/// the new state without a second read.
#[async_trait]
pub trait PinsPort: Send + Sync {
    async fn load(&self) -> Vec<Pin>;
    async fn add(&self, pin: Pin) -> Result<Vec<Pin>, FilesError>;
    async fn remove(&self, path: &str) -> Result<Vec<Pin>, FilesError>;
}

/// Persistence for the explorer's per-user preferences. `load` never fails: a
/// missing or unreadable store yields [`FilePreferences::default`], so the
/// explorer always has a usable configuration. `save` reports an input/output
/// failure so a caller can surface it to the user.
#[async_trait]
pub trait PreferencesPort: Send + Sync {
    async fn load(&self) -> FilePreferences;
    async fn save(&self, preferences: FilePreferences) -> Result<(), FilesError>;
}

/// Source of the applications offered by the explorer's "Open with" menu.
/// Infrastructure implements this over the desktop-entry scan.
#[async_trait]
pub trait ApplicationCatalog: Send + Sync {
    async fn list_applications(&self) -> Vec<ApplicationInfo>;
}

/// Streams process snapshots while at least one watcher is registered. Sampling
/// is reference-counted: `watch` registers interest and returns a stream of
/// snapshots. Interest is released when that stream is dropped (which drops the
/// underlying subscription); the monitor stops sampling once no stream remains.
/// `unwatch` is an explicit hook for implementations that track registrations
/// out of band, and may be a no-op when dropping the stream already releases
/// the registration. Synchronous: registering a watch does not yield.
pub trait ProcessMonitor: Send + Sync {
    fn watch(&self) -> BoxStream<'static, ProcessSnapshot>;
    fn unwatch(&self);
}

/// Streams the pointer position while at least one subscriber listens.
/// Mirrors `ProcessMonitor`: `watch` hands out a stream that resumes the
/// underlying poll; dropping it lets the poll idle. `unwatch` is a no-op.
pub trait CursorMonitor: Send + Sync {
    fn watch(&self) -> BoxStream<'static, CursorPosition>;
    fn unwatch(&self);
}

/// Signals a process and its whole subtree. Implementations resolve the subtree
/// and deliver `signal` to each member, refusing to kill protected processes.
#[async_trait]
pub trait ProcessKiller: Send + Sync {
    async fn kill_subtree(&self, pid: i32, signal: KillSignal) -> Result<(), ProcessesError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeProvider {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for FakeProvider {
        fn id(&self) -> &ProviderId {
            &self.id
        }

        async fn search(&self, _q: &Query) -> Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }

        async fn invoke(&self, _a: &Action) -> Result<ActionOutcome, DomainError> {
            Ok(ActionOutcome { message: None })
        }
    }

    #[tokio::test]
    async fn fake_provider_returns_empty_search() {
        let p = FakeProvider {
            id: ProviderId::from("apps"),
        };
        let q = Query::new("x");
        let r = p.search(&q).await.unwrap();
        assert!(r.is_empty());
    }

    #[tokio::test]
    async fn fake_provider_has_correct_id() {
        let p = FakeProvider {
            id: ProviderId::from("test-provider"),
        };
        assert_eq!(p.id(), &ProviderId::from("test-provider"));
    }
}

#[cfg(test)]
mod subscribe_tests {
    use super::*;
    use futures::stream::{self, StreamExt};

    struct FakeSubscriber {
        id: ProviderId,
    }

    #[async_trait]
    impl ProviderSource for FakeSubscriber {
        fn id(&self) -> &ProviderId {
            &self.id
        }
        async fn search(&self, _: &Query) -> Result<Vec<Match>, DomainError> {
            Ok(vec![])
        }
        async fn invoke(&self, _: &Action) -> Result<ActionOutcome, DomainError> {
            Ok(ActionOutcome { message: None })
        }
        fn subscribe(&self) -> Option<futures::stream::BoxStream<'static, serde_json::Value>> {
            Some(stream::iter(vec![serde_json::json!({"x": 1})]).boxed())
        }
    }

    #[tokio::test]
    async fn subscribe_returns_stream_when_supported() {
        let p = FakeSubscriber { id: "fake".into() };
        let mut stream = p.subscribe().expect("stream");
        let event = stream.next().await.expect("event");
        assert_eq!(event, serde_json::json!({"x": 1}));
    }
}

#[cfg(test)]
mod input_region_tests {
    use super::*;

    #[test]
    fn window_input_region_json_round_trips() {
        let region = WindowInputRegion {
            x: 10,
            y: 20,
            width: 300,
            height: 32,
        };
        let json = serde_json::to_string(&region).expect("serialize");
        let parsed: WindowInputRegion = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, region);
    }
}

#[cfg(test)]
mod timer_port_tests {
    use super::*;

    // Compile-time proof that all four timer ports are object-safe and can be
    // used behind `Arc<dyn Trait>`. If any trait stopped being object-safe,
    // this would fail to compile.
    #[allow(dead_code)]
    fn assert_object_safe(
        _clock: Arc<dyn Clock>,
        _store: Arc<dyn TimerStore>,
        _notifier: Arc<dyn TimerNotifier>,
        _broadcast: Arc<dyn TimerBroadcast>,
    ) {
    }

    #[test]
    fn timer_ports_are_object_safe() {
        let _: Option<Arc<dyn Clock>> = None;
        let _: Option<Arc<dyn TimerStore>> = None;
        let _: Option<Arc<dyn TimerNotifier>> = None;
        let _: Option<Arc<dyn TimerBroadcast>> = None;
    }
}

#[cfg(test)]
mod notification_emitter_tests {
    use super::*;

    // Compile-time proof that the notification-emitter port is object-safe and
    // can be used behind `Arc<dyn Trait>`. If the trait stopped being
    // object-safe, this would fail to compile.
    #[allow(dead_code)]
    fn assert_object_safe(_emitter: Arc<dyn NotificationEmitter>) {}

    #[test]
    fn notification_emitter_port_is_object_safe() {
        let _: Option<Arc<dyn NotificationEmitter>> = None;
    }
}

#[cfg(test)]
mod process_port_tests {
    use super::*;

    // Compile-time proof that both process ports are object-safe and can be
    // used behind `Arc<dyn Trait>`. If either trait stopped being object-safe,
    // this would fail to compile.
    #[allow(dead_code)]
    fn assert_object_safe(_monitor: Arc<dyn ProcessMonitor>, _killer: Arc<dyn ProcessKiller>) {}

    #[test]
    fn process_ports_are_object_safe() {
        let _: Option<Arc<dyn ProcessMonitor>> = None;
        let _: Option<Arc<dyn ProcessKiller>> = None;
    }
}

#[cfg(test)]
mod filesystem_port_tests {
    use super::*;

    #[test]
    fn size_update_round_trips_through_serde() {
        let update = SizeUpdate {
            path: "/home/user/projects".to_string(),
            bytes: 4_096,
            complete: false,
        };
        let json = serde_json::to_value(&update).expect("serialize");
        assert_eq!(json["path"], "/home/user/projects");
        assert_eq!(json["bytes"], 4_096);
        assert_eq!(json["complete"], false);
        let back: SizeUpdate = serde_json::from_value(json).expect("round trip");
        assert_eq!(back, update);
    }

    // Compile-time proof that all four filesystem ports are object-safe and can
    // be used behind `Arc<dyn Trait>`. If any trait stopped being object-safe,
    // this would fail to compile.
    #[allow(dead_code)]
    fn assert_object_safe(
        _filesystem: Arc<dyn FileSystemPort>,
        _watcher: Arc<dyn DirectoryWatcher>,
        _opener: Arc<dyn FileOpener>,
        _sizer: Arc<dyn RecursiveSizer>,
    ) {
    }

    #[test]
    fn filesystem_ports_are_object_safe() {
        let _: Option<Arc<dyn FileSystemPort>> = None;
        let _: Option<Arc<dyn DirectoryWatcher>> = None;
        let _: Option<Arc<dyn FileOpener>> = None;
        let _: Option<Arc<dyn RecursiveSizer>> = None;
    }

    // Compile-time proof that the pins and application-catalog ports are
    // object-safe and can be used behind `Arc<dyn Trait>`.
    #[allow(dead_code)]
    fn assert_explorer_ports_object_safe(
        _pins: Arc<dyn PinsPort>,
        _applications: Arc<dyn ApplicationCatalog>,
    ) {
    }

    #[test]
    fn explorer_ports_are_object_safe() {
        let _: Option<Arc<dyn PinsPort>> = None;
        let _: Option<Arc<dyn ApplicationCatalog>> = None;
    }
}
