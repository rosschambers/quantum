// Pure helpers for the sidebar drive gauges. Kept in a plain module (not a
// `.svelte.ts`) so they carry no reactive state and are trivially unit-testable.
// Both functions are deterministic given their input.

import type { DriveInfo } from '@quantum/client';

/**
 * The fraction of a drive that is used, in the range 0..1. Computed from free
 * and total space (used = 1 - free / total). A zero-size drive reports 0 rather
 * than dividing by zero.
 */
export function driveUsedFraction(drive: DriveInfo): number {
    if (drive.total_bytes === 0) {
        return 0;
    }
    return 1 - drive.free_bytes / drive.total_bytes;
}

/** The severity class for a drive's fill bar, given its used fraction (0..1). */
export type DriveBarClass = 'normal' | 'warn' | 'crit';

/**
 * Classify a drive's used fraction into a fill-bar severity: critical above 90
 * percent used, a warning above 75 percent, otherwise normal.
 */
export function driveBarClass(fraction: number): DriveBarClass {
    if (fraction > 0.9) {
        return 'crit';
    }
    if (fraction > 0.75) {
        return 'warn';
    }
    return 'normal';
}
