// Pure month-grid model for the clock calendar dropdown. No dates are read
// from the system here: every function is a pure function of its arguments, so
// the grid is deterministic and testable. Display-only client-side date math;
// there is no daemon calendar state (v1).

/** A single cell in the six-week month grid. */
export interface DayCell {
    day: number;
    inMonth: boolean;
}

const MONTH_NAMES = [
    'January',
    'February',
    'March',
    'April',
    'May',
    'June',
    'July',
    'August',
    'September',
    'October',
    'November',
    'December',
];

/** Total cells in the grid: six weeks of seven days. */
const TOTAL_CELLS = 42;

/**
 * Build a flat array of forty-two cells (six weeks by seven days), the week
 * starting on Sunday, for the given year and zero-based month (0 is January).
 *
 * Leading cells are the tail of the previous month (`inMonth: false`), the
 * target month's days follow (`inMonth: true`), and trailing cells fill to
 * forty-two with the start of the next month (`inMonth: false`).
 */
export function monthGrid(year: number, month0: number): DayCell[] {
    // Day-of-week of the first of the month, zero (Sunday) through six.
    const firstDayOfWeek = new Date(year, month0, 1).getDay();
    // Day zero of the next month resolves to the last day of this month.
    const daysInMonth = new Date(year, month0 + 1, 0).getDate();
    // Day zero of this month resolves to the last day of the previous month.
    const daysInPreviousMonth = new Date(year, month0, 0).getDate();

    const cells: DayCell[] = [];

    // Leading cells: the tail of the previous month, counting up to its last
    // day so the first in-month cell lands on the correct weekday column.
    for (let index = 0; index < firstDayOfWeek; index += 1) {
        const day = daysInPreviousMonth - firstDayOfWeek + 1 + index;
        cells.push({ day, inMonth: false });
    }

    // In-month cells.
    for (let day = 1; day <= daysInMonth; day += 1) {
        cells.push({ day, inMonth: true });
    }

    // Trailing cells: the start of the next month, filling to forty-two.
    let nextDay = 1;
    while (cells.length < TOTAL_CELLS) {
        cells.push({ day: nextDay, inMonth: false });
        nextDay += 1;
    }

    return cells;
}

/**
 * True only for the in-month cell whose year, month, and day match `now`. An
 * out-of-month cell is never today even when its day number coincides, because
 * it belongs to an adjacent month.
 */
export function isToday(
    cell: DayCell,
    cellYear: number,
    cellMonth0: number,
    now: Date,
): boolean {
    return (
        cell.inMonth &&
        cellYear === now.getFullYear() &&
        cellMonth0 === now.getMonth() &&
        cell.day === now.getDate()
    );
}

/** Render a month label such as "January 2026". */
export function monthLabel(year: number, month0: number): string {
    return `${MONTH_NAMES[month0]} ${year}`;
}
