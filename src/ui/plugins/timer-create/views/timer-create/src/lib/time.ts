/** A half-day period for a 12-hour clock display. */
export type Period = 'am' | 'pm';

/**
 * Convert a 12-hour clock hour (1-12) plus a period into a 24-hour hour
 * (0-23). 12 AM is midnight (0); 12 PM is noon (12).
 */
export function to24Hour(hour12: number, period: Period): number {
    const base = hour12 % 12;
    return period === 'pm' ? base + 12 : base;
}

/**
 * Convert a 24-hour hour (0-23) into its 12-hour clock representation: an hour
 * in 1-12 and a period. 0 is 12 AM; 12 is 12 PM.
 */
export function to12Hour(hour24: number): { hour12: number; period: Period } {
    const period: Period = hour24 < 12 ? 'am' : 'pm';
    const remainder = hour24 % 12;
    const hour12 = remainder === 0 ? 12 : remainder;
    return { hour12, period };
}

/**
 * Set the period of a 24-hour hour, preserving the clock hour. Switching to
 * `am` moves an afternoon hour back twelve hours; switching to `pm` moves a
 * morning hour forward twelve hours.
 */
export function setPeriod(hour24: number, period: Period): number {
    const { hour12 } = to12Hour(hour24);
    return to24Hour(hour12, period);
}
