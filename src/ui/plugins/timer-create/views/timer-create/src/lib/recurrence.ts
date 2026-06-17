import type { Weekday } from '@quantum/client';

/** The seven weekdays in `monday..sunday` order, used for the `daily` case
 * and as the canonical ordering for the custom multi-select. */
export const ALL_WEEKDAYS: Weekday[] = [
    'monday',
    'tuesday',
    'wednesday',
    'thursday',
    'friday',
    'saturday',
    'sunday',
];

/** The recurrence mode chosen in the form. */
export type Recurrence = 'none' | 'daily' | 'custom';

/**
 * Resolve the recurrence selection into the `days` array for a recurring
 * timer, or `null` when the timer is not recurring (`none`) or the custom
 * selection is empty.
 */
export function recurrenceDays(
    recurrence: Recurrence,
    customDays: Weekday[],
): Weekday[] | null {
    if (recurrence === 'daily') return [...ALL_WEEKDAYS];
    if (recurrence === 'custom') return customDays.length > 0 ? customDays : null;
    return null;
}
