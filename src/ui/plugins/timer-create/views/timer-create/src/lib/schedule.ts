import type { TimeOfDay, Weekday } from '@quantum/client';
import { ALL_WEEKDAYS } from './recurrence';

/** The Monday-through-Friday subset, used to recognise the "Weekdays" set. */
const WEEKDAYS_MONDAY_TO_FRIDAY: Weekday[] = [
    'monday',
    'tuesday',
    'wednesday',
    'thursday',
    'friday',
];

/** Three-letter display labels for each weekday. */
const WEEKDAY_LABELS: Record<Weekday, string> = {
    monday: 'Mon',
    tuesday: 'Tue',
    wednesday: 'Wed',
    thursday: 'Thu',
    friday: 'Fri',
    saturday: 'Sat',
    sunday: 'Sun',
};

/**
 * Format a remaining duration in seconds as `m:ss` (under an hour) or
 * `h:mm:ss` (an hour or more). Negative inputs clamp to zero so an expired
 * one-shot reads `0:00`.
 */
export function formatRemaining(secs: number): string {
    const clamped = Math.max(0, Math.floor(secs));
    const hours = Math.floor(clamped / 3600);
    const minutes = Math.floor((clamped % 3600) / 60);
    const seconds = clamped % 60;
    if (hours > 0) {
        return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
    }
    return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

/** Format a wall-clock time of day as zero-padded `HH:MM`. */
export function formatClock(time: TimeOfDay): string {
    return `${String(time.hour).padStart(2, '0')}:${String(time.minute).padStart(2, '0')}`;
}

/**
 * Summarise a recurring timer's day set: `Daily` for all seven days,
 * `Weekdays` for exactly Monday through Friday, otherwise a comma-separated
 * list of three-letter day labels in canonical order.
 */
export function summarizeDays(days: Weekday[]): string {
    const selected = new Set(days);
    if (selected.size === 7) return 'Daily';
    if (
        selected.size === WEEKDAYS_MONDAY_TO_FRIDAY.length &&
        WEEKDAYS_MONDAY_TO_FRIDAY.every((day) => selected.has(day))
    ) {
        return 'Weekdays';
    }
    return ALL_WEEKDAYS.filter((day) => selected.has(day))
        .map((day) => WEEKDAY_LABELS[day])
        .join(', ');
}
