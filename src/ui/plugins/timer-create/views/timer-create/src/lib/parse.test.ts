import { describe, it, expect } from 'vitest';
import { parseDurationToSecs, parseTimeOfDay } from './parse';

describe('parseDurationToSecs', () => {
    it('parses minutes', () => {
        expect(parseDurationToSecs('45m')).toBe(2700);
    });

    it('parses combined hours and minutes', () => {
        expect(parseDurationToSecs('1h30m')).toBe(5400);
    });

    it('parses seconds', () => {
        expect(parseDurationToSecs('90s')).toBe(90);
    });

    it('parses bare hours', () => {
        expect(parseDurationToSecs('2h')).toBe(7200);
    });

    it('returns null for unparseable input', () => {
        expect(parseDurationToSecs('banana')).toBeNull();
    });

    it('returns null for a zero total', () => {
        expect(parseDurationToSecs('0m')).toBeNull();
    });
});

describe('parseTimeOfDay', () => {
    it('parses a valid HH:MM', () => {
        expect(parseTimeOfDay('17:15')).toEqual({ hour: 17, minute: 15 });
    });

    it('returns null for an out-of-range hour', () => {
        expect(parseTimeOfDay('25:00')).toBeNull();
    });

    it('returns null for an out-of-range minute', () => {
        expect(parseTimeOfDay('10:75')).toBeNull();
    });

    it('returns null for malformed input', () => {
        expect(parseTimeOfDay('noon')).toBeNull();
    });
});
