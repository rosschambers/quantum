/**
 * Nerd Font icon codepoints used across the bar.
 *
 * The default theme expects a Nerd Font in `--font-mono` / fallback
 * (e.g. JetBrainsMono Nerd Font). Plain unicode emoji and symbol
 * glyphs were unreliable across the WebKit / system font stack —
 * some rendered as full-color emoji, others as missing-glyph
 * rectangles, others not at all. Nerd Font codepoints are all in
 * the Private Use Area and ship as proper monochrome icons that
 * inherit the CSS `color`, so every bar indicator renders in the
 * bar's foreground regardless of which font fallback wins.
 *
 * Codepoint sources: Font Awesome (`nf-fa-*`) and Material Design
 * Icons (`nf-md-*`). See https://www.nerdfonts.com/cheat-sheet.
 */

export const Icons = {
    // System meters
    cpu: '\u{f2db}', // fa-microchip
    memory: '\u{efc5}', // fa-memory

    // Battery — Material Design vertical battery glyphs in 10%
    // buckets. fa-battery_* renders as a horizontal pill which is
    // hard to read inside a small ring; md variants are vertical.
    batteryEmpty: '\u{f008e}', // md-battery_outline
    battery10: '\u{f007a}', // md-battery_10
    battery20: '\u{f007b}', // md-battery_20
    battery30: '\u{f007c}', // md-battery_30
    battery40: '\u{f007d}', // md-battery_40
    battery50: '\u{f007e}', // md-battery_50
    battery60: '\u{f007f}', // md-battery_60
    battery70: '\u{f0080}', // md-battery_70
    battery80: '\u{f0081}', // md-battery_80
    battery90: '\u{f0082}', // md-battery_90
    batteryFull: '\u{f0079}', // md-battery
    batteryCharging: '\u{f0084}', // md-battery_charging

    // Audio
    volumeOff: '\u{f026}',
    volumeLow: '\u{f027}',
    volumeHigh: '\u{f028}',

    // Brightness. The obvious candidates all have problems in
    // JetBrainsMono Nerd Font: fa-sun (f185) renders as a gear, and
    // md-weather_sunny (f0599) renders as a dotted-outline circle.
    // md-brightness_5 (f00de) is the right glyph — a sun with rays
    // explicitly labelled "brightness" in the Material Design set.
    brightness: '\u{f00de}',

    // Network
    wifi: '\u{f1eb}', // fa-wifi
    ethernet: '\u{f0e8}', // fa-plug
    signal: '\u{f012}', // fa-signal
    networkOff: '\u{f00d}', // fa-times for missing

    // Bluetooth
    bluetooth: '\u{f293}', // fa-bluetooth

    // Power profile — leaf / arrow / bolt triad
    leaf: '\u{f06c}', // fa-leaf — power saver
    balanced: '\u{f061}', // fa-arrow_right — balanced
    bolt: '\u{f0e7}', // fa-bolt — performance

    // Power menu / system
    powerMenu: '\u{f0425}', // md-power_settings_new
    chevronDown: '\u{f078}', // fa-chevron_down
};

/**
 * Pick a battery glyph based on percent + charging state. Charging
 * always renders as the bolt-overlaid battery; otherwise the bucket
 * matches the nearest 10%.
 */
export function batteryIcon(percent: number | null, charging: boolean): string {
    if (charging) return Icons.batteryCharging;
    if (percent === null) return Icons.batteryEmpty;
    if (percent >= 95) return Icons.batteryFull;
    if (percent >= 85) return Icons.battery90;
    if (percent >= 75) return Icons.battery80;
    if (percent >= 65) return Icons.battery70;
    if (percent >= 55) return Icons.battery60;
    if (percent >= 45) return Icons.battery50;
    if (percent >= 35) return Icons.battery40;
    if (percent >= 25) return Icons.battery30;
    if (percent >= 15) return Icons.battery20;
    if (percent >= 5) return Icons.battery10;
    return Icons.batteryEmpty;
}

/**
 * Pick a volume glyph based on mute state + percent.
 */
export function volumeIcon(percent: number, muted: boolean): string {
    if (muted || percent === 0) return Icons.volumeOff;
    if (percent < 50) return Icons.volumeLow;
    return Icons.volumeHigh;
}

/**
 * Pick a network glyph based on the connection kind. Falls back to a
 * generic signal glyph for unknown / cellular.
 */
export function networkIcon(kind: string | null, hasPrimary: boolean): string {
    if (!hasPrimary) return Icons.networkOff;
    if (kind === 'ethernet') return Icons.ethernet;
    if (kind === 'wifi') return Icons.wifi;
    return Icons.signal;
}

/**
 * Pick a power-profile glyph for the active profile.
 */
export function powerProfileIcon(profile: string | null): string {
    if (profile === 'power_saver') return Icons.leaf;
    if (profile === 'performance') return Icons.bolt;
    return Icons.balanced;
}
