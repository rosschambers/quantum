export interface WindowListEntry {
  address: string;
  class: string;
  title: string;
  workspace_id: number;
  workspace_name: string;
}

export interface WindowList {
  windows: WindowListEntry[];
}

export interface SystemStats {
  cpu_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
}

export type PlaybackStatus = 'playing' | 'paused' | 'stopped';

export interface MprisState {
  player_id: string | null;
  title: string | null;
  artist: string | null;
  album: string | null;
  art_url: string | null;
  playback_status: PlaybackStatus;
  position_micros: number | null;
  length_micros: number | null;
}

export interface ActiveWindowState {
  title: string;
  class: string;
  workspace_id: number;
  workspace_name: string;
}

export interface MonitorActiveWindowState {
  monitors: Record<string, ActiveWindowState>;
  focused_monitor: string | null;
}

export type BatteryChargeState = 'charging' | 'discharging' | 'full' | 'empty' | 'unknown';

export interface PowerState {
  available: boolean;
  on_battery: boolean;
  percentage: number | null;
  state: BatteryChargeState | null;
  time_to_empty_secs: number | null;
  time_to_full_secs: number | null;
}

export type NetworkConnectivity = 'none' | 'portal' | 'limited' | 'full' | 'unknown';
export type NetworkKind = 'ethernet' | 'wifi' | 'cellular' | 'vpn' | 'other';

export interface NetworkConnection {
  kind: NetworkKind;
  id: string;
  ssid: string | null;
}

export interface NetworkState {
  available: boolean;
  connectivity: NetworkConnectivity;
  primary: NetworkConnection | null;
  wifi_enabled: boolean;
  wifi_signal_percent: number | null;
}

export interface BluetoothDevice {
  address: string;
  name: string;
  battery_percent: number | null;
  paired: boolean;
  trusted: boolean;
  connected: boolean;
  icon: string | null;
  rssi: number | null;
}

export interface BluetoothState {
  available: boolean;
  powered: boolean;
  discovering: boolean;
  devices: BluetoothDevice[];
  adapter_path: string;
}

export type PowerProfile = 'power_saver' | 'balanced' | 'performance';

export interface PowerProfileState {
  available: boolean;
  active: PowerProfile | null;
  profiles: PowerProfile[];
  performance_inhibited: string | null;
}

export interface AudioSink {
  name: string;
  description: string;
  volume_percent: number;
  muted: boolean;
}

export interface AudioDevice {
  index: number;
  name: string;
  description: string;
  volume_percent: number;
  muted: boolean;
  is_default: boolean;
  port: string | null;
}

export interface AudioStream {
  index: number;
  application_name: string;
  media_name: string;
  icon: string | null;
  volume_percent: number;
  muted: boolean;
  device_index: number;
}

export interface AudioCardProfile {
  name: string;
  description: string;
  available: boolean;
}

export interface AudioCard {
  index: number;
  name: string;
  description: string;
  active_profile: string;
  profiles: AudioCardProfile[];
}

export interface AudioState {
  available: boolean;
  default_sink: AudioSink | null;
  /** The default input device (microphone), reusing the sink shape. */
  default_source?: AudioSink | null;
  /** Extended state for the sound window; optional because the bar only
   *  reads the defaults. */
  sinks?: AudioDevice[];
  sources?: AudioDevice[];
  playback_streams?: AudioStream[];
  recording_streams?: AudioStream[];
  cards?: AudioCard[];
}

export interface BrightnessDisplay {
  subsystem: string;
  name: string;
  current: number;
  max: number;
}

export interface BrightnessState {
  available: boolean;
  displays: BrightnessDisplay[];
}

export interface SystemPowerState {
  can_shutdown: boolean;
  can_restart: boolean;
  can_suspend: boolean;
  can_hibernate: boolean;
  can_lock: boolean;
}
