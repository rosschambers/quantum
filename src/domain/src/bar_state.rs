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
    /// Every known `org.bluez.Device1` object: connected,
    /// paired-but-disconnected, and discovered-unpaired.
    #[serde(default)]
    pub devices: Vec<BluetoothDevice>,
    /// The DBus object path of the selected adapter, resolved dynamically.
    /// Empty when no adapter is present.
    #[serde(default)]
    pub adapter_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothDevice {
    pub address: String,
    pub name: String,
    pub battery_percent: Option<u8>,
    #[serde(default)]
    pub paired: bool,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default)]
    pub connected: bool,
    /// BlueZ device-class icon name (for example "audio-headset").
    #[serde(default)]
    pub icon: Option<String>,
    /// Received signal strength, present while the device is in discovery range.
    #[serde(default)]
    pub rssi: Option<i16>,
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
    /// The default input device (microphone), when one is present. Reuses
    /// [`AudioSink`] because a source carries the same name/description/volume/
    /// mute fields. Used by the bar to show and toggle microphone mute.
    #[serde(default)]
    pub default_source: Option<AudioSink>,
    /// Every output device, for the sound window's Output devices section.
    #[serde(default)]
    pub sinks: Vec<AudioDevice>,
    /// Every real input device (monitor sources filtered out).
    #[serde(default)]
    pub sources: Vec<AudioDevice>,
    /// Sink-inputs. Populated only while a sound-window session is open.
    #[serde(default)]
    pub playback_streams: Vec<AudioStream>,
    /// Source-outputs. Populated only while a sound-window session is open.
    #[serde(default)]
    pub recording_streams: Vec<AudioStream>,
    /// Cards with their switchable profiles.
    #[serde(default)]
    pub cards: Vec<AudioCard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSink {
    pub name: String,
    pub description: String,
    pub volume_percent: u8,
    pub muted: bool,
}

/// One audio device (sink or source) in the sound window's device lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDevice {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub volume_percent: u8,
    pub muted: bool,
    pub is_default: bool,
    /// Description of the active port (for example "Speaker"), when the
    /// device reports one. Shown as the row subtitle.
    pub port: Option<String>,
}

/// One playback (sink-input) or recording (source-output) stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStream {
    pub index: u32,
    pub application_name: String,
    pub media_name: String,
    pub icon: Option<String>,
    pub volume_percent: u8,
    pub muted: bool,
    /// Index of the sink (playback) or source (recording) the stream is
    /// currently routed to.
    pub device_index: u32,
}

/// One switchable profile on a card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioCardProfile {
    pub name: String,
    pub description: String,
    pub available: bool,
    /// Number of output sinks this profile exposes. Zero means the profile
    /// produces no sound (the sound window surfaces this as "0 out = no sound").
    pub sink_count: u32,
    /// Number of input sources this profile exposes.
    pub source_count: u32,
}

/// One sound card with its profiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioCard {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub active_profile: String,
    pub profiles: Vec<AudioCardProfile>,
}

/// Security type of a WiFi network. Distinguishes WPA versions for the
/// text label; the connect flow treats all personal variants identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiSecurity {
    Open,
    Wpa,
    Wpa2,
    Wpa3,
    Other,
}

/// WiFi frequency band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiBand {
    TwoFour,
    Five,
    Six,
    Unknown,
}

/// IPv4 configuration method for a saved connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ipv4Method {
    Auto,
    Manual,
}

/// Read-only details of the active connection (advanced details panel).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WifiConnectionDetails {
    pub ip_address: Option<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub mac: Option<String>,
    pub frequency_mhz: Option<u32>,
    pub ipv4_method: Ipv4Method,
    pub metered: bool,
}

/// The currently-connected WiFi network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ActiveWifi {
    pub ssid: String,
    pub signal_percent: u8,
    pub security: WifiSecurity,
    pub details: Option<WifiConnectionDetails>,
}

/// One row in the scan list. Empty `ssid` means a hidden network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: String,
    pub signal_percent: u8,
    pub security: WifiSecurity,
    pub band: WifiBand,
    pub saved: bool,
    pub active: bool,
}

/// A stored connection profile (saved-networks list).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SavedNetwork {
    pub id: String,
    pub ssid: String,
    pub security: WifiSecurity,
    pub autoconnect: bool,
    pub in_range: bool,
}

/// Full WiFi state the overlay renders from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WifiState {
    pub available: bool,
    pub radio_enabled: bool,
    pub scanning: bool,
    pub active: Option<ActiveWifi>,
    pub networks: Vec<WifiNetwork>,
    pub saved: Vec<SavedNetwork>,
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
            devices: vec![BluetoothDevice {
                address: "AA:BB:CC:DD:EE:FF".into(),
                name: "Headphones".into(),
                battery_percent: Some(60),
                paired: true,
                trusted: true,
                connected: true,
                icon: Some("audio-headset".into()),
                rssi: Some(-42),
            }],
            adapter_path: "/org/bluez/hci0".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        let back: BluetoothState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn bluetooth_state_deserializes_legacy_payload_without_new_fields() {
        let v = json!({"available": true, "powered": true, "discovering": false});
        let s: BluetoothState = serde_json::from_value(v).unwrap();
        assert!(s.devices.is_empty());
        assert_eq!(s.adapter_path, "");
    }

    #[test]
    fn bluetooth_device_deserializes_without_new_fields() {
        let v = json!({
            "address": "AA:BB:CC:DD:EE:FF",
            "name": "Headphones",
            "battery_percent": null
        });
        let d: BluetoothDevice = serde_json::from_value(v).unwrap();
        assert!(!d.paired);
        assert!(!d.trusted);
        assert!(!d.connected);
        assert_eq!(d.icon, None);
        assert_eq!(d.rssi, None);
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
            default_source: Some(AudioSink {
                name: "alsa_input.pci-0000_00_1f.3.analog-stereo".into(),
                description: "Built-in Microphone".into(),
                volume_percent: 80,
                muted: true,
            }),
            sinks: vec![],
            sources: vec![],
            playback_streams: vec![],
            recording_streams: vec![],
            cards: vec![],
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
    fn extended_audio_state_round_trips() {
        let s = AudioState {
            available: true,
            default_sink: Some(AudioSink {
                name: "alsa_output.speaker".into(),
                description: "Speaker".into(),
                volume_percent: 55,
                muted: false,
            }),
            default_source: None,
            sinks: vec![AudioDevice {
                index: 59,
                name: "alsa_output.speaker".into(),
                description: "Speaker".into(),
                volume_percent: 55,
                muted: false,
                is_default: true,
                port: Some("Speaker".into()),
            }],
            sources: vec![AudioDevice {
                index: 61,
                name: "alsa_input.microphone".into(),
                description: "Digital Microphone".into(),
                volume_percent: 100,
                muted: true,
                is_default: true,
                port: None,
            }],
            playback_streams: vec![AudioStream {
                index: 900,
                application_name: "paplay".into(),
                media_name: "/dev/zero".into(),
                icon: None,
                volume_percent: 100,
                muted: false,
                device_index: 59,
            }],
            recording_streams: vec![],
            cards: vec![AudioCard {
                index: 48,
                name: "alsa_card.pci".into(),
                description: "Arrow Lake cAVS".into(),
                active_profile: "HiFi".into(),
                profiles: vec![AudioCardProfile {
                    name: "HiFi".into(),
                    description: "Play HiFi quality Music".into(),
                    available: true,
                    sink_count: 1,
                    source_count: 0,
                }],
            }],
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["sinks"][0]["is_default"], json!(true));
        assert_eq!(v["cards"][0]["profiles"][0]["available"], json!(true));
        let back: AudioState = serde_json::from_value(v).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn audio_state_without_new_fields_still_deserializes() {
        let v = json!({
            "available": true,
            "default_sink": {
                "name": "s",
                "description": "Built-in",
                "volume_percent": 65,
                "muted": false
            },
            "default_source": null
        });
        let back: AudioState = serde_json::from_value(v).unwrap();
        assert!(back.available);
        assert!(back.sinks.is_empty());
        assert!(back.sources.is_empty());
        assert!(back.playback_streams.is_empty());
        assert!(back.recording_streams.is_empty());
        assert!(back.cards.is_empty());
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

    #[test]
    fn wifi_state_round_trips_through_serde() {
        let state = WifiState {
            available: true,
            radio_enabled: true,
            scanning: true,
            active: Some(ActiveWifi {
                ssid: "Skynet_5G".to_string(),
                signal_percent: 92,
                security: WifiSecurity::Wpa3,
                details: None,
            }),
            networks: vec![WifiNetwork {
                ssid: "Skynet_5G".to_string(),
                bssid: "a1:b2:c3:d4:e5:f6".to_string(),
                signal_percent: 92,
                security: WifiSecurity::Wpa3,
                band: WifiBand::Five,
                saved: true,
                active: true,
            }],
            saved: vec![SavedNetwork {
                id: "Skynet_5G".to_string(),
                ssid: "Skynet_5G".to_string(),
                security: WifiSecurity::Wpa3,
                autoconnect: true,
                in_range: true,
            }],
        };
        let json = serde_json::to_string(&state).expect("serialize");
        let back: WifiState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(state, back);
    }

    #[test]
    fn wifi_band_two_four_serializes_snake_case() {
        let json = serde_json::to_string(&WifiBand::TwoFour).expect("serialize");
        assert_eq!(json, "\"two_four\"");
    }

    #[test]
    fn wifi_state_default_is_unavailable_and_empty() {
        let d = WifiState::default();
        assert!(!d.available);
        assert!(!d.radio_enabled);
        assert!(d.networks.is_empty());
        assert!(d.saved.is_empty());
    }
}
