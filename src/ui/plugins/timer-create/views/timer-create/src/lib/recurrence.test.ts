import { describe, it, expect } from 'vitest';
import type { Weekday } from '@quantum/client';
import { recurrenceDays } from './recurrence';

describe('recurrenceDays', () => {
    it('daily yields all seven days in monday..sunday order', () => {
        expect(recurrenceDays('daily', [])).toEqual([
            'monday',
            'tuesday',
            'wednesday',
            'thursday',
            'friday',
            'saturday',
            'sunday',
        ]);
    });

    it('custom passes the selected days through', () => {
        const selected: Weekday[] = ['monday', 'wednesday', 'friday'];
        expect(recurrenceDays('custom', selected)).toEqual(selected);
    });

    it('custom with no selection returns null', () => {
        expect(recurrenceDays('custom', [])).toBeNull();
    });

    it('none returns null', () => {
        expect(recurrenceDays('none', ['monday'])).toBeNull();
    });
});
