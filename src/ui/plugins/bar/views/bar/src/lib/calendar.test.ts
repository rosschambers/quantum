import { describe, it, expect } from 'vitest';
import { monthGrid, isToday, monthLabel, type DayCell } from './calendar';

describe('monthGrid', () => {
    it('lays out July 2026 (whose first day is a Wednesday) with three leading cells', () => {
        // July 1 2026 falls on a Wednesday. With a week starting on Sunday that
        // leaves three leading out-of-month cells (Sunday, Monday, Tuesday).
        const grid = monthGrid(2026, 6);

        const leading = grid.filter((cell, index) => index < 3);
        expect(leading.every((cell) => !cell.inMonth)).toBe(true);
        expect(grid[3]).toEqual({ day: 1, inMonth: true });
    });

    it('has thirty-one in-month cells for July and a total length of forty-two', () => {
        const grid = monthGrid(2026, 6);

        expect(grid.filter((cell) => cell.inMonth)).toHaveLength(31);
        expect(grid).toHaveLength(42);
    });

    it('fills leading cells with the tail of the previous month', () => {
        // June 2026 has thirty days, so the three leading cells count up to it:
        // 28, 29, 30.
        const grid = monthGrid(2026, 6);

        expect(grid.slice(0, 3)).toEqual([
            { day: 28, inMonth: false },
            { day: 29, inMonth: false },
            { day: 30, inMonth: false },
        ]);
    });

    it('fills trailing cells with the start of the next month', () => {
        const grid = monthGrid(2026, 6);
        // Three leading plus thirty-one in-month is thirty-four cells, leaving
        // eight trailing cells that count 1..8 from August.
        const trailing = grid.slice(34);
        expect(trailing).toHaveLength(8);
        expect(trailing).toEqual([
            { day: 1, inMonth: false },
            { day: 2, inMonth: false },
            { day: 3, inMonth: false },
            { day: 4, inMonth: false },
            { day: 5, inMonth: false },
            { day: 6, inMonth: false },
            { day: 7, inMonth: false },
            { day: 8, inMonth: false },
        ]);
    });

    it('counts twenty-nine in-month cells for February in a leap year (2028)', () => {
        const grid = monthGrid(2028, 1);
        expect(grid.filter((cell) => cell.inMonth)).toHaveLength(29);
        expect(grid).toHaveLength(42);
    });

    it('counts twenty-eight in-month cells for February in a non-leap year (2027)', () => {
        const grid = monthGrid(2027, 1);
        expect(grid.filter((cell) => cell.inMonth)).toHaveLength(28);
        expect(grid).toHaveLength(42);
    });
});

describe('isToday', () => {
    it('flags exactly one cell when the grid month contains today', () => {
        const now = new Date(2026, 6, 15, 9, 30, 0);
        const grid = monthGrid(2026, 6);

        const flagged = grid.filter((cell) => isToday(cell, 2026, 6, now));
        expect(flagged).toHaveLength(1);
        expect(flagged[0]).toEqual({ day: 15, inMonth: true });
    });

    it('never flags an out-of-month cell even when its day number matches', () => {
        // A leading cell can carry the same day number as today; it must not be
        // flagged because it belongs to the previous month.
        const now = new Date(2026, 6, 30, 12, 0, 0);
        const leadingCell: DayCell = { day: 30, inMonth: false };
        expect(isToday(leadingCell, 2026, 6, now)).toBe(false);
    });

    it('flags no cell when the displayed month is not the current month', () => {
        const now = new Date(2026, 6, 15, 9, 30, 0);
        const grid = monthGrid(2026, 7);

        const flagged = grid.filter((cell) => isToday(cell, 2026, 7, now));
        expect(flagged).toHaveLength(0);
    });
});

describe('monthLabel', () => {
    it('renders the English month name and year', () => {
        expect(monthLabel(2026, 0)).toBe('January 2026');
        expect(monthLabel(2026, 6)).toBe('July 2026');
        expect(monthLabel(2026, 11)).toBe('December 2026');
    });
});
