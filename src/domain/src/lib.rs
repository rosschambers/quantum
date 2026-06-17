//! quantum-domain

pub mod action;
pub mod bar_state;
pub mod error;
pub mod event_bus;
pub mod ids;
pub mod match_result;
pub mod notifications;
pub mod ports;
pub mod query;
pub mod score;
pub mod timer;
pub mod tokens;
pub mod view_descriptor;
pub mod window_mode;

pub use action::Action;
pub use notifications::{Notification, NotificationEvent, NotificationUrgency};
pub use bar_state::{
    ActiveWifi, ActiveWindowState, AudioSink, AudioState, BatteryChargeState, BluetoothDevice,
    BluetoothState, BrightnessDisplay, BrightnessState, Ipv4Method, MonitorActiveWindowState,
    MprisState, NetworkConnection, NetworkConnectivity, NetworkKind, NetworkState, PlaybackStatus,
    PowerProfile, PowerProfileState, PowerState, SavedNetwork, SystemPowerState, SystemStats,
    WifiBand, WifiConnectionDetails, WifiNetwork, WifiSecurity, WifiState,
};
pub use error::DomainError;
pub use event_bus::EventEnvelope;
pub use ids::ProviderId;
pub use match_result::{IconRef, Match};
pub use ports::{
    ActionOutcome, Clock, EventBus, HyprlandClient, PluginCatalog, ProviderRegistry,
    ProviderSource, ShellExecutor, ShellOutput, ThemeStore, TimerBroadcast, TimerNotifier,
    TimerStore, WindowHost,
};
pub use query::Query;
pub use score::MatchScore;
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
