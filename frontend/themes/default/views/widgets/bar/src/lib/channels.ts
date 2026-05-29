// Provider event channel constants. Keep in lockstep with the provider
// ids registered in `crates/bin/quantumd/src/main.rs`. The daemon
// publishes on `format!("{provider_id}.event")` so the strings here must
// match each id exactly. A typo here trips TypeScript instead of
// silently no-subscribing.

export const SYSTEM_STATS_CHANNEL = 'system.stats.event';
export const MPRIS_CHANNEL = 'mpris.event';
export const ACTIVE_WINDOW_CHANNEL = 'hyprland.activewindow.event';

export const POWER_CHANNEL = 'power.event';
export const NETWORK_CHANNEL = 'network.event';
export const BLUETOOTH_CHANNEL = 'bluetooth.event';
export const POWER_PROFILE_CHANNEL = 'power_profile.event';
export const AUDIO_CHANNEL = 'audio.event';
export const BRIGHTNESS_CHANNEL = 'brightness.event';
