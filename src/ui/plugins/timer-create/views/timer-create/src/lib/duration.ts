/** A quick-pick duration preset: a value in seconds and its short label. */
export interface DurationPreset {
    secs: number;
    label: string;
}

/**
 * The approved quick-pick duration presets, ascending. Note 60m and 1h are the
 * same 3600 seconds; the chip set uses 1h. The 2h chip is 7200 seconds.
 */
export const DURATION_PRESETS: DurationPreset[] = [
    { secs: 300, label: '5m' },
    { secs: 600, label: '10m' },
    { secs: 900, label: '15m' },
    { secs: 1800, label: '30m' },
    { secs: 2700, label: '45m' },
    { secs: 3600, label: '1h' },
    { secs: 7200, label: '2h' },
];

/** The smallest duration a custom stepper may reach, in seconds (one minute). */
export const MINIMUM_DURATION_SECS = 60;

/**
 * Format a duration in seconds as a compact human label: `45m`, `1h`,
 * `1h 30m`, `2h`. Minutes are rounded to the nearest whole minute.
 */
export function formatDuration(secs: number): string {
    const hours = Math.floor(secs / 3600);
    const minutes = Math.round((secs % 3600) / 60);
    if (hours > 0 && minutes > 0) return `${hours}h ${minutes}m`;
    if (hours > 0) return `${hours}h`;
    return `${minutes}m`;
}

/**
 * Step a duration by `deltaSecs`, clamped so the result is never below the
 * one-minute minimum.
 */
export function stepDuration(secs: number, deltaSecs: number): number {
    return Math.max(MINIMUM_DURATION_SECS, secs + deltaSecs);
}
