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
}

export interface BluetoothState {
  available: boolean;
  powered: boolean;
  discovering: boolean;
  connected_devices: BluetoothDevice[];
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

export interface AudioState {
  available: boolean;
  default_sink: AudioSink | null;
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
