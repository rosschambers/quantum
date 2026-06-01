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

    // Battery — buckets approximated by quartile
    batteryEmpty: '\u{f244}', // fa-battery_0
    batteryQuarter: '\u{f243}', // fa-battery_1
    batteryHalf: '\u{f242}', // fa-battery_2
    batteryThreeQuarters: '\u{f241}', // fa-battery_3
    batteryFull: '\u{f240}', // fa-battery_4
    batteryCharging: '\u{f0084}', // md-battery_charging

    // Audio
    volumeOff: '\u{f026}',
    volumeLow: '\u{f027}',
    volumeHigh: '\u{f028}',

    // Brightness — md-weather_sunny renders as a clear sun across all
    // Nerd Font variants. fa-sun (f185) maps to a gear-shaped glyph in
    // JetBrains Mono Nerd Font, which read wrong as "brightness".
    brightness: '\u{f0599}',

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
 * matches the nearest quartile.
 */
export function batteryIcon(percent: number | null, charging: boolean): string {
    if (charging) return Icons.batteryCharging;
    if (percent === null) return Icons.batteryEmpty;
    if (percent >= 90) return Icons.batteryFull;
    if (percent >= 65) return Icons.batteryThreeQuarters;
    if (percent >= 40) return Icons.batteryHalf;
    if (percent >= 15) return Icons.batteryQuarter;
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
