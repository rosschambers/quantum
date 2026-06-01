import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import ScheduleExpander from './ScheduleExpander.svelte';

function defaultProps(
    overrides: Partial<Parameters<typeof render>[1]['props']> = {},
) {
    return {
        open: true,
        onSchedule: vi.fn().mockResolvedValue(undefined),
        onNow: vi.fn(),
        onClose: vi.fn(),
        ...overrides,
    };
}

describe('ScheduleExpander', () => {
    it('renders nothing when closed', () => {
        const props = defaultProps({ open: false });
        const { container } = render(ScheduleExpander, { props });
        expect(container.querySelector('.schedule-expander')).toBeNull();
    });

    it('renders the preset row when open', () => {
        const props = defaultProps();
        const { container } = render(ScheduleExpander, { props });
        const expander = container.querySelector('.schedule-expander');
        expect(expander).not.toBeNull();
        const pills = Array.from(container.querySelectorAll('.pill')).map(
            (p) => p.textContent?.trim() ?? '',
        );
        expect(pills).toContain('Now');
        expect(pills).toContain('5m');
        expect(pills).toContain('15m');
        expect(pills).toContain('30m');
        expect(pills).toContain('1h');
        expect(pills).toContain('Custom…');
    });

    it('clicking Now calls onNow and onClose, never onSchedule', async () => {
        const props = defaultProps();
        const { container } = render(ScheduleExpander, { props });
        const nowPill = Array.from(container.querySelectorAll('.pill')).find(
            (p) => p.textContent?.trim() === 'Now',
        ) as HTMLElement;
        await fireEvent.click(nowPill);
        await tick();
        expect(props.onNow).toHaveBeenCalledTimes(1);
        expect(props.onClose).toHaveBeenCalledTimes(1);
        expect(props.onSchedule).not.toHaveBeenCalled();
    });

    it('clicking a preset pill calls onSchedule with the correct delay', async () => {
        const props = defaultProps();
        const { container } = render(ScheduleExpander, { props });
        const fifteen = container.querySelector('[data-preset="15m"]') as HTMLElement;
        await fireEvent.click(fifteen);
        await tick();
        expect(props.onSchedule).toHaveBeenCalledWith(15 * 60);
        expect(props.onClose).toHaveBeenCalledTimes(1);
    });

    it('Custom button reveals the input row', async () => {
        const props = defaultProps();
        const { container } = render(ScheduleExpander, { props });
        const custom = Array.from(container.querySelectorAll('.pill')).find(
            (p) => p.textContent?.trim() === 'Custom…',
        ) as HTMLElement;
        await fireEvent.click(custom);
        await tick();
        expect(container.querySelector('.custom-row')).not.toBeNull();
        expect(container.querySelector('.custom-input')).not.toBeNull();
        expect(container.querySelector('.preset-row')).toBeNull();
    });

    it('custom input clamps to 1-1440 minutes on submit', async () => {
        const props = defaultProps();
        const { container } = render(ScheduleExpander, { props });
        // Switch to custom mode
        const custom = Array.from(container.querySelectorAll('.pill')).find(
            (p) => p.textContent?.trim() === 'Custom…',
        ) as HTMLElement;
        await fireEvent.click(custom);
        await tick();

        // Set input to 2000 (above clamp)
        const input = container.querySelector('.custom-input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: '2000' } });
        await tick();

        const submit = Array.from(container.querySelectorAll('.pill')).find(
            (p) => p.textContent?.trim() === 'Schedule',
        ) as HTMLElement;
        await fireEvent.click(submit);
        await tick();
        await tick();
        // 1440 minutes = 86400 seconds (the daemon's max)
        expect(props.onSchedule).toHaveBeenCalledWith(1440 * 60);
    });

    it('custom Cancel button reverts to preset row', async () => {
        const props = defaultProps();
        const { container } = render(ScheduleExpander, { props });
        const custom = Array.from(container.querySelectorAll('.pill')).find(
            (p) => p.textContent?.trim() === 'Custom…',
        ) as HTMLElement;
        await fireEvent.click(custom);
        await tick();
        const cancel = Array.from(container.querySelectorAll('.pill')).find(
            (p) => p.textContent?.trim() === 'Cancel',
        ) as HTMLElement;
        await fireEvent.click(cancel);
        await tick();
        expect(container.querySelector('.preset-row')).not.toBeNull();
        expect(container.querySelector('.custom-row')).toBeNull();
        expect(props.onClose).not.toHaveBeenCalled();
        expect(props.onSchedule).not.toHaveBeenCalled();
    });
});
