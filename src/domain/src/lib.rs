//! quantum-domain

pub mod action;
pub mod bar_state;
pub mod clipboard;
pub mod cursor;
pub mod error;
pub mod event_bus;
pub mod files;
pub mod ids;
pub mod match_result;
pub mod notifications;
pub mod ports;
pub mod processes;
pub mod query;
pub mod score;
pub mod shell_capture;
pub mod system_tray;
pub mod timer;
pub mod tokens;
pub mod view_descriptor;
pub mod window_mode;

pub use action::Action;
pub use bar_state::{
    ActiveWifi, ActiveWindowState, AudioCard, AudioCardProfile, AudioDevice, AudioSink, AudioState,
    AudioStream, BatteryChargeState, BluetoothDevice, BluetoothState, BrightnessDisplay,
    BrightnessState, Ipv4Method, MonitorActiveWindowState, MprisState, NetworkConnection,
    NetworkConnectivity, NetworkKind, NetworkState, PlaybackStatus, PowerProfile,
    PowerProfileState, PowerState, SavedNetwork, SystemPowerState, SystemStats, WifiBand,
    WifiConnectionDetails, WifiNetwork, WifiSecurity, WifiState,
};
pub use clipboard::{ClipboardData, ClipboardEntry, ClipboardError};
pub use cursor::CursorPosition;
pub use error::DomainError;
pub use event_bus::EventEnvelope;
pub use files::{
    classify_permissions, content_kind_for_name, ApplicationInfo, ContentKind, DriveInfo,
    FileEntry, FileEntryKind, FileOperation, FilePreferences, FilesError, PermissionClass, Pin,
};
pub use ids::ProviderId;
pub use match_result::{IconRef, Match, MenuAction};
pub use notifications::{Notification, NotificationEvent, NotificationUrgency};
pub use ports::{
    ActionOutcome, ApplicationCatalog, ClipboardStore, ClipboardWriter, Clock, CursorMonitor,
    DirectoryWatcher, EventBus, FileOpener, FileSystemPort, HyprlandClient, NotificationEmitter,
    PinsPort, PluginCatalog, PreferencesPort, ProcessKiller, ProcessMonitor, ProviderRegistry,
    ProviderSource, RecursiveSizer, ShellExecutor, ShellOutput, SizeUpdate, ThemeStore,
    TimerBroadcast, TimerNotifier, TimerStore, WindowHost, WindowInputRegion,
};
pub use processes::{
    build_forest, collect_subtree_pids, GlobalStats, KillSignal, ProcessNode, ProcessSnapshot,
    ProcessesError, RawProcess, WindowInfo,
};
pub use query::Query;
pub use score::MatchScore;
pub use shell_capture::ShellCaptureResult;
pub use system_tray::{SystemTrayItem, SystemTrayMenuNode, SystemTrayState};
pub use timer::{
    seconds_until_next, CivilNow, NotifyConfig, Point, SoundName, TextColor, TextPosition,
    TextVisibility, TimeFormat, TimeOfDay, Timer, TimerError, TimerId, TimerKind, TimerSettings,
    TimerStatus, TimerStoreData, VisualConfig, VisualStyle, Weekday, WeekdaySet,
};
pub use tokens::tokens_to_css;
pub use view_descriptor::{ViewAnchor, ViewDescriptor, ViewKind, ViewPosition};
pub use window_mode::WindowMode;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
