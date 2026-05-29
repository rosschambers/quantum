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
