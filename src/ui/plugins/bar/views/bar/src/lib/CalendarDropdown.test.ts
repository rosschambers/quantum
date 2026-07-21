import { describe, it, expect, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import CalendarDropdown from './CalendarDropdown.svelte';

// A fixed reference date keeps every assertion deterministic: 15 July 2026,
// a month whose first day is a Wednesday. Passing it as `initialDate`
// decouples the component from the real clock.
const REFERENCE = new Date(2026, 6, 15, 9, 30, 0);

afterEach(() => cleanup());

function header(container: HTMLElement): string {
    return container.querySelector('.month')?.textContent?.trim() ?? '';
}

function todayCell(container: HTMLElement): HTMLElement | null {
    return container.querySelector('.day.today');
}

describe('CalendarDropdown', () => {
    it('renders the initial month with today flagged', async () => {
        const { container } = render(CalendarDropdown, {
            props: { initialDate: REFERENCE },
        });
        await tick();

        expect(header(container)).toBe('July 2026');
        const today = todayCell(container);
        expect(today).not.toBeNull();
        expect(today?.textContent?.trim()).toBe('15');
    });

    it('renders the seven weekday headers starting on Sunday', async () => {
        const { container } = render(CalendarDropdown, {
            props: { initialDate: REFERENCE },
        });
        await tick();

        const labels = Array.from(container.querySelectorAll('.dow')).map((el) =>
            el.textContent?.trim(),
        );
        expect(labels).toEqual(['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa']);
    });

    it('advances to the next month when next is clicked', async () => {
        const { container } = render(CalendarDropdown, {
            props: { initialDate: REFERENCE },
        });
        await tick();

        await fireEvent.click(container.querySelector('.next') as HTMLButtonElement);
        await tick();

        expect(header(container)).toBe('August 2026');
        // August is not the reference month, so no cell is today.
        expect(todayCell(container)).toBeNull();
    });

    it('goes to the previous month when previous is clicked', async () => {
        const { container } = render(CalendarDropdown, {
            props: { initialDate: REFERENCE },
        });
        await tick();

        await fireEvent.click(container.querySelector('.prev') as HTMLButtonElement);
        await tick();

        expect(header(container)).toBe('June 2026');
    });

    it('returns to the current month when Today is clicked', async () => {
        const { container } = render(CalendarDropdown, {
            props: { initialDate: REFERENCE },
        });
        await tick();

        await fireEvent.click(container.querySelector('.next') as HTMLButtonElement);
        await fireEvent.click(container.querySelector('.next') as HTMLButtonElement);
        await tick();
        expect(header(container)).toBe('September 2026');

        await fireEvent.click(container.querySelector('.today-btn') as HTMLButtonElement);
        await tick();

        expect(header(container)).toBe('July 2026');
        expect(todayCell(container)?.textContent?.trim()).toBe('15');
    });

    it('advances the month on a downward wheel and rewinds on an upward wheel', async () => {
        const { container } = render(CalendarDropdown, {
            props: { initialDate: REFERENCE },
        });
        await tick();

        const grid = container.querySelector('.grid') as HTMLElement;

        await fireEvent.wheel(grid, { deltaY: 120 });
        await tick();
        expect(header(container)).toBe('August 2026');

        await fireEvent.wheel(grid, { deltaY: -120 });
        await tick();
        expect(header(container)).toBe('July 2026');
    });
});
