//! quantum-domain

pub mod action;
pub mod bar_state;
pub mod error;
pub mod event_bus;
pub mod ids;
pub mod match_result;
pub mod ports;
pub mod query;
pub mod score;
pub mod tokens;
pub mod view_descriptor;
pub mod window_mode;

pub use action::Action;
pub use bar_state::{
    ActiveWindowState, AudioSink, AudioState, BatteryChargeState, BluetoothDevice, BluetoothState,
    BrightnessDisplay, BrightnessState, MonitorActiveWindowState, MprisState, NetworkConnection,
    NetworkConnectivity, NetworkKind, NetworkState, PlaybackStatus, PowerProfile,
    PowerProfileState, PowerState, SystemPowerState, SystemStats,
};
pub use error::DomainError;
pub use event_bus::EventEnvelope;
pub use ids::ProviderId;
pub use match_result::{IconRef, Match};
pub use ports::{
    ActionOutcome, EventBus, HyprlandClient, PluginCatalog, ProviderRegistry, ProviderSource,
    ShellExecutor, ShellOutput, ThemeStore, WindowHost,
};
pub use query::Query;
pub use score::MatchScore;
pub use tokens::tokens_to_css;
pub use view_descriptor::{ViewAnchor, ViewDescriptor, ViewKind};
pub use window_mode::WindowMode;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
