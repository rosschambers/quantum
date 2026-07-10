// Provider id and event channel constants. Keep in lockstep with the
// provider ids registered in `crates/bin/quantumd/src/main.rs`. The
// daemon publishes on `format!("{provider_id}.event")` so the channel
// string for each provider is always `<provider_id>.event`. Centralizing
// both halves here means a future provider rename only touches one file,
// and a typo trips TypeScript instead of silently no-subscribing or
// no-querying.

export const SYSTEM_STATS_PROVIDER = 'system.stats';
export const SYSTEM_STATS_CHANNEL = 'system.stats.event';

export const MPRIS_PROVIDER = 'mpris';
export const MPRIS_CHANNEL = 'mpris.event';

export const ACTIVE_WINDOW_PROVIDER = 'hyprland.activewindow';
export const ACTIVE_WINDOW_CHANNEL = 'hyprland.activewindow.event';

export const POWER_PROVIDER = 'power';
export const POWER_CHANNEL = 'power.event';

export const NETWORK_PROVIDER = 'network';
export const NETWORK_CHANNEL = 'network.event';

// The Wi-Fi overlay's own provider. The bar opens that overlay, so it starts
// the provider's scan session on open; the overlay ends it on dismiss.
export const WIFI_PROVIDER = 'wifi';

export const BLUETOOTH_PROVIDER = 'bluetooth';
export const BLUETOOTH_CHANNEL = 'bluetooth.event';

export const POWER_PROFILE_PROVIDER = 'power_profile';
export const POWER_PROFILE_CHANNEL = 'power_profile.event';

export const AUDIO_PROVIDER = 'audio';
export const AUDIO_CHANNEL = 'audio.event';

export const BRIGHTNESS_PROVIDER = 'brightness';
export const BRIGHTNESS_CHANNEL = 'brightness.event';

export const SYSTEM_POWER_PROVIDER = 'system_power';
export const SYSTEM_POWER_CHANNEL = 'system_power.event';

export const SYSTEM_TRAY_PROVIDER = 'system_tray';
export const SYSTEM_TRAY_CHANNEL = 'system_tray.event';
