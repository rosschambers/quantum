use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemStats {
    pub cpu_percent: f32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MprisState {
    pub player_id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub art_url: Option<String>,
    pub playback_status: PlaybackStatus,
    pub position_micros: Option<u64>,
    pub length_micros: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveWindowState {
    pub title: String,
    pub class: String,
    pub workspace_id: i64,
    pub workspace_name: String,
}

/// Per-monitor focused-window snapshot. Keyed by Hyprland monitor
/// connector name (for example "DP-1", "eDP-1"). Single-monitor
/// systems have exactly one entry; multi-monitor systems have one
/// entry per connected output.
///
/// UIs that don't know their own monitor (single-monitor systems,
/// or a custom widget without `__quantum_monitor` set) read the
/// entry under `focused_monitor`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorActiveWindowState {
    pub monitors: HashMap<String, ActiveWindowState>,
    pub focused_monitor: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PowerState {
    pub available: bool,
    pub on_battery: bool,
    pub percentage: Option<f32>,
    pub state: Option<BatteryChargeState>,
    pub time_to_empty_secs: Option<u64>,
    pub time_to_full_secs: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryChargeState {
    Charging,
    Discharging,
    Full,
    Empty,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkState {
    pub available: bool,
    pub connectivity: NetworkConnectivity,
    pub primary: Option<NetworkConnection>,
    pub wifi_enabled: bool,
    pub wifi_signal_percent: Option<u8>,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            available: false,
            connectivity: NetworkConnectivity::Unknown,
            primary: None,
            wifi_enabled: false,
            wifi_signal_percent: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkConnectivity {
    None,
    Portal,
    Limited,
    Full,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkConnection {
    pub kind: NetworkKind,
    pub id: String,
    pub ssid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkKind {
    Ethernet,
    Wifi,
    Cellular,
    Vpn,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothState {
    pub available: bool,
    pub powered: bool,
    pub discovering: bool,
    pub connected_devices: Vec<BluetoothDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothDevice {
    pub address: String,
    pub name: String,
    pub battery_percent: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerProfileState {
    pub available: bool,
    pub active: Option<PowerProfile>,
    pub profiles: Vec<PowerProfile>,
    pub performance_inhibited: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerProfile {
    PowerSaver,
    Balanced,
    Performance,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioState {
    pub available: bool,
    pub default_sink: Option<AudioSink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSink {
    pub name: String,
    pub description: String,
    pub volume_percent: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrightnessState {
    pub available: bool,
    pub displays: Vec<BrightnessDisplay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrightnessDisplay {
    pub subsystem: String,
    pub name: String,
    pub current: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemPowerState {
    pub can_shutdown: bool,
    pub can_restart: bool,
    pub can_suspend: bool,
    pub can_hibernate: bool,
    pub can_lock: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn system_stats_round_trips() {
        let s = SystemStats {
            cpu_percent: 12.5,
            mem_used_bytes: 1024,
            mem_total_bytes: 4096,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v,
            json!({"cpu_percent": 12.5, "mem_used_bytes": 1024, "mem_total_bytes": 4096})
        );
        let back: SystemStats = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn playback_status_serializes_snake_case() {
        let v = serde_json::to_value(PlaybackStatus::Playing).unwrap();
        assert_eq!(v, json!("playing"));
    }

    #[test]
    fn mpris_state_with_no_player_round_trips() {
        let s = MprisState {
            player_id: None,
            title: None,
            artist: None,
            album: None,
            art_url: None,
            playback_status: PlaybackStatus::Stopped,
            position_micros: None,
            length_micros: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: MprisState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn active_window_state_round_trips() {
        let s = ActiveWindowState {
            title: "Mozilla Firefox".into(),
            class: "firefox".into(),
            workspace_id: 1,
            workspace_name: "1".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: ActiveWindowState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn power_state_round_trips() {
        let s = PowerState {
            available: true,
            on_battery: true,
            percentage: Some(72.5),
            state: Some(BatteryChargeState::Discharging),
            time_to_empty_secs: Some(3600),
            time_to_full_secs: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: PowerState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn power_state_default_is_unavailable() {
        assert!(!PowerState::default().available);
    }

    #[test]
    fn network_state_round_trips() {
        let s = NetworkState {
            available: true,
            connectivity: NetworkConnectivity::Full,
            primary: Some(NetworkConnection {
                kind: NetworkKind::Wifi,
                id: "home".into(),
                ssid: Some("HomeWifi".into()),
            }),
            wifi_enabled: true,
            wifi_signal_percent: Some(82),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["connectivity"], json!("full"));
        assert_eq!(v["primary"]["kind"], json!("wifi"));
        let back: NetworkState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn network_state_default_is_unavailable() {
        assert!(!NetworkState::default().available);
    }

    #[test]
    fn bluetooth_state_round_trips() {
        let s = BluetoothState {
            available: true,
            powered: true,
            discovering: false,
            connected_devices: vec![BluetoothDevice {
                address: "AA:BB:CC:DD:EE:FF".into(),
                name: "Headphones".into(),
                battery_percent: Some(60),
            }],
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: BluetoothState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn bluetooth_state_default_is_unavailable() {
        assert!(!BluetoothState::default().available);
    }

    #[test]
    fn power_profile_state_round_trips() {
        let s = PowerProfileState {
            available: true,
            active: Some(PowerProfile::Balanced),
            profiles: vec![
                PowerProfile::PowerSaver,
                PowerProfile::Balanced,
                PowerProfile::Performance,
            ],
            performance_inhibited: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["active"], json!("balanced"));
        let back: PowerProfileState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn power_profile_state_default_is_unavailable() {
        assert!(!PowerProfileState::default().available);
    }

    #[test]
    fn audio_state_round_trips() {
        let s = AudioState {
            available: true,
            default_sink: Some(AudioSink {
                name: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
                description: "Built-in Audio".into(),
                volume_percent: 65,
                muted: false,
            }),
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: AudioState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn audio_state_default_is_unavailable() {
        assert!(!AudioState::default().available);
    }

    #[test]
    fn brightness_state_round_trips() {
        let s = BrightnessState {
            available: true,
            displays: vec![BrightnessDisplay {
                subsystem: "backlight".into(),
                name: "intel_backlight".into(),
                current: 50000,
                max: 96000,
            }],
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: BrightnessState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn brightness_state_default_is_unavailable() {
        assert!(!BrightnessState::default().available);
    }

    #[test]
    fn system_power_state_round_trips() {
        let s = SystemPowerState {
            can_shutdown: true,
            can_restart: true,
            can_suspend: true,
            can_hibernate: false,
            can_lock: true,
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: SystemPowerState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn system_power_state_default_all_false() {
        let d = SystemPowerState::default();
        assert!(
            !d.can_shutdown && !d.can_restart && !d.can_suspend && !d.can_hibernate && !d.can_lock
        );
    }

    #[test]
    fn monitor_active_window_state_default_is_empty() {
        let s = MonitorActiveWindowState::default();
        assert!(s.monitors.is_empty());
        assert_eq!(s.focused_monitor, None);
    }

    #[test]
    fn monitor_active_window_state_round_trips_with_one_monitor() {
        let mut s = MonitorActiveWindowState::default();
        s.monitors.insert(
            "DP-1".into(),
            ActiveWindowState {
                class: "firefox".into(),
                title: "Mozilla Firefox".into(),
                workspace_id: 1,
                workspace_name: "1".into(),
            },
        );
        s.focused_monitor = Some("DP-1".into());
        let v = serde_json::to_value(&s).unwrap();
        let back: MonitorActiveWindowState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn monitor_active_window_state_round_trips_with_multiple_monitors() {
        let mut s = MonitorActiveWindowState::default();
        s.monitors
            .insert("DP-1".into(), ActiveWindowState::default());
        s.monitors
            .insert("HDMI-A-1".into(), ActiveWindowState::default());
        let v = serde_json::to_value(&s).unwrap();
        let back: MonitorActiveWindowState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }
}
