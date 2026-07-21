//! quantum-providers: ProviderSource implementations.

pub mod error;

pub mod registry;
pub mod shell;
pub mod shell_notification_emitter;

pub mod app_usage;
pub mod audio;
pub mod battery;
pub mod bluetooth;
pub mod bluetooth_agent;
pub mod brightness;
pub mod calc;
pub mod clipboard_store;
pub mod clipboard_writer;
pub mod declarative_shell;
pub mod desktop_apps;
pub mod emoji;
pub mod hyprland_active_window;
pub mod hyprland_windows;
pub mod mpris;
pub mod network;
pub mod notifications;
pub mod plugin_script;
pub mod power_profile;
pub mod proc_stats;
pub mod shell_command;
pub mod system_power;
pub mod system_tray;
pub mod timer;
pub mod timer_clock;
pub mod timer_notifier;
pub mod timer_store;
pub mod wifi;

pub use error::ProvidersError;
pub use registry::InMemoryProviderRegistry;
pub use shell::TokioShellExecutor;
pub use shell_notification_emitter::ProviderNotificationEmitter;

pub use audio::PulseAudioProvider;
pub use battery::UpowerBatteryProvider;
pub use bluetooth::BluezProvider;
pub use brightness::LogindBrightnessProvider;
pub use calc::CalcProvider;
pub use clipboard_store::FileClipboardStore;
pub use clipboard_writer::WlClipboardWriter;
pub use declarative_shell::DeclarativeShellProvider;
pub use desktop_apps::DesktopAppsProvider;
pub use emoji::EmojiProvider;
pub use hyprland_active_window::HyprlandActiveWindowProvider;
pub use hyprland_windows::HyprlandWindowsProvider;
pub use mpris::MprisProvider;
pub use network::NetworkManagerProvider;
pub use notifications::NotificationsProvider;
pub use plugin_script::PluginScriptProvider;
pub use power_profile::PowerProfilesDaemonProvider;
pub use proc_stats::ProcStatsProvider;
pub use shell_command::ShellCommandProvider;
pub use system_power::SystemPowerProvider;
pub use system_tray::SystemTrayProvider;
pub use timer::TimerProvider;
pub use timer_clock::SystemClock;
pub use timer_notifier::{NotificationTimerNotifier, SoundPlayer};
pub use timer_store::JsonTimerStore;
pub use wifi::WifiProvider;
