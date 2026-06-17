import { describe, it, expect } from 'vitest';
import { to12Hour, to24Hour, setPeriod } from './time';

describe('to24Hour', () => {
    it('maps 12 AM to 0', () => {
        expect(to24Hour(12, 'am')).toBe(0);
    });

    it('maps 12 PM to 12', () => {
        expect(to24Hour(12, 'pm')).toBe(12);
    });

    it('maps 1 PM to 13', () => {
        expect(to24Hour(1, 'pm')).toBe(13);
    });

    it('maps 8 AM to 8', () => {
        expect(to24Hour(8, 'am')).toBe(8);
    });
});

describe('to12Hour', () => {
    it('maps 0 to 12 AM', () => {
        expect(to12Hour(0)).toEqual({ hour12: 12, period: 'am' });
    });

    it('maps 12 to 12 PM', () => {
        expect(to12Hour(12)).toEqual({ hour12: 12, period: 'pm' });
    });

    it('maps 13 to 1 PM', () => {
        expect(to12Hour(13)).toEqual({ hour12: 1, period: 'pm' });
    });

    it('maps 8 to 8 AM', () => {
        expect(to12Hour(8)).toEqual({ hour12: 8, period: 'am' });
    });
});

describe('setPeriod', () => {
    it('keeps a morning hour morning', () => {
        expect(setPeriod(8, 'am')).toBe(8);
    });

    it('flips a morning hour to afternoon', () => {
        expect(setPeriod(8, 'pm')).toBe(20);
    });

    it('flips an afternoon hour to morning', () => {
        expect(setPeriod(20, 'am')).toBe(8);
    });

    it('keeps an afternoon hour afternoon', () => {
        expect(setPeriod(20, 'pm')).toBe(20);
    });
});
