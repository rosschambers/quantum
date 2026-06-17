import { describe, it, expect } from 'vitest';
import { DURATION_PRESETS, formatDuration, stepDuration } from './duration';

describe('DURATION_PRESETS', () => {
    it('lists the seven approved presets in ascending order', () => {
        expect(DURATION_PRESETS.map((preset) => preset.secs)).toEqual([
            300, 600, 900, 1800, 2700, 3600, 7200,
        ]);
    });

    it('labels 3600 as 1h and 7200 as 2h', () => {
        const oneHour = DURATION_PRESETS.find((preset) => preset.secs === 3600);
        const twoHours = DURATION_PRESETS.find((preset) => preset.secs === 7200);
        expect(oneHour?.label).toBe('1h');
        expect(twoHours?.label).toBe('2h');
    });
});

describe('formatDuration', () => {
    it('formats whole minutes', () => {
        expect(formatDuration(2700)).toBe('45m');
    });

    it('formats a whole hour without minutes', () => {
        expect(formatDuration(3600)).toBe('1h');
    });

    it('formats hours and minutes together', () => {
        expect(formatDuration(5400)).toBe('1h 30m');
    });

    it('formats multiple whole hours', () => {
        expect(formatDuration(7200)).toBe('2h');
    });
});

describe('stepDuration', () => {
    it('adds the delta', () => {
        expect(stepDuration(2700, 300)).toBe(3000);
    });

    it('subtracts the delta', () => {
        expect(stepDuration(2700, -300)).toBe(2400);
    });

    it('clamps to a one-minute minimum', () => {
        expect(stepDuration(60, -300)).toBe(60);
    });
});
