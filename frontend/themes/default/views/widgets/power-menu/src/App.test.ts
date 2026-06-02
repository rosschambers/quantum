import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';

/*
 * Module-level mock state. Each test resets these via beforeEach so
 * the `vi.mock` factory below sees fresh values. The factory closes
 * over them by reference.
 */
let mockCapabilities: Record<string, boolean> = {
    can_shutdown: true,
    can_restart: true,
    can_suspend: true,
    can_hibernate: false,
    can_lock: true,
};
let mockCallSpy = vi.fn();
let mockSubscribeSpy = vi.fn();

vi.mock('@quantum/client', () => ({
    createClient: () => ({
        call: (...args: unknown[]) => {
            mockCallSpy(...args);
            const [method, params] = args as [string, unknown];
            if (method === 'provider.query' && (params as { id?: string })?.id === 'system_power') {
                return Promise.resolve(mockCapabilities);
            }
            if (method === 'action.scheduled') {
                return Promise.resolve({ jobs: [] });
            }
            return Promise.resolve(undefined);
        },
        subscribe: (...args: unknown[]) => {
            mockSubscribeSpy(...args);
            return () => {};
        },
        close: vi.fn(),
    }),
    __esModule: true,
}));

import App from './App.svelte';

beforeEach(() => {
    mockCallSpy = vi.fn();
    mockSubscribeSpy = vi.fn();
    mockCapabilities = {
        can_shutdown: true,
        can_restart: true,
        can_suspend: true,
        can_hibernate: false,
        can_lock: true,
    };
});

async function settle(): Promise<void> {
    // Two animation frames + a microtask flush is enough for the
    // provider.query promise chain and the subsequent state assignment
    // to propagate through Svelte's effect scheduler.
    await new Promise((resolve) => setTimeout(resolve, 10));
    await tick();
    await tick();
}

describe('PowerMenu App', () => {
    it('renders the backdrop and the centered card', () => {
        const { container } = render(App);
        expect(container.querySelector('.backdrop')).not.toBeNull();
        expect(container.querySelector('.card')).not.toBeNull();
    });

    it('does NOT render a header bar with title or close button', async () => {
        const { container } = render(App);
        await settle();
        // The new design drops the header entirely; users dismiss via
        // Escape or by clicking outside the card.
        expect(container.querySelector('.card-header')).toBeNull();
        expect(container.querySelector('.close')).toBeNull();
    });

    it('queries the system_power provider on mount', async () => {
        render(App);
        await settle();
        const queried = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'provider.query' &&
                (params as { id?: string })?.id === 'system_power',
        );
        expect(queried).toBe(true);
    });

    it('subscribes to system_power.event on mount', async () => {
        render(App);
        await settle();
        const subscribed = mockSubscribeSpy.mock.calls.some(
            ([channel]) => channel === 'system_power.event',
        );
        expect(subscribed).toBe(true);
    });

    it('renders an ActionTile only for each capability that is true', async () => {
        const { container } = render(App);
        await settle();
        const tiles = Array.from(container.querySelectorAll('[data-command]')).map(
            (tile) => tile.getAttribute('data-command') ?? '',
        );
        expect(tiles).toContain('shutdown');
        expect(tiles).toContain('restart');
        expect(tiles).toContain('suspend');
        expect(tiles).toContain('lock');
        // can_hibernate is false in the mocked capabilities.
        expect(tiles).not.toContain('hibernate');
    });

    it('shutdown and restart land in the primary tier, others in secondary', async () => {
        const { container } = render(App);
        await settle();
        const primary = container.querySelector('.tier-primary') as HTMLElement;
        const secondary = container.querySelector('.tier-secondary') as HTMLElement;
        expect(primary).not.toBeNull();
        expect(secondary).not.toBeNull();
        expect(primary.querySelector('[data-command="shutdown"]')).not.toBeNull();
        expect(primary.querySelector('[data-command="restart"]')).not.toBeNull();
        expect(secondary.querySelector('[data-command="suspend"]')).not.toBeNull();
        expect(secondary.querySelector('[data-command="lock"]')).not.toBeNull();
    });

    it('renders the DelayPicker at the bottom of the card', async () => {
        const { container } = render(App);
        await settle();
        const picker = container.querySelector('.delay-picker');
        expect(picker).not.toBeNull();
    });

    it('fetches the scheduled jobs list on mount', async () => {
        render(App);
        await settle();
        const queried = mockCallSpy.mock.calls.some(([method]) => method === 'action.scheduled');
        expect(queried).toBe(true);
    });

    it('two clicks on an action with delay=Now routes to action.invoke', async () => {
        const { container } = render(App);
        await settle();
        const shutdown = container.querySelector('[data-command="shutdown"]') as HTMLElement;
        await fireEvent.click(shutdown); // arm
        await tick();
        await fireEvent.click(shutdown); // confirm
        await tick();
        await tick();
        const invokeCall = mockCallSpy.mock.calls.find(([method]) => method === 'action.invoke');
        expect(invokeCall).toBeDefined();
        const params = invokeCall![1] as {
            action?: { data?: { payload?: { command?: string } } };
        };
        expect(params.action?.data?.payload?.command).toBe('shutdown');
        // No action.schedule call when delay is Now.
        const scheduleCall = mockCallSpy.mock.calls.find(
            ([method]) => method === 'action.schedule',
        );
        expect(scheduleCall).toBeUndefined();
    });

    it('two clicks on an action with delay>0 routes to action.schedule', async () => {
        const { container } = render(App);
        await settle();

        // Open the delay picker popover and choose 15 min.
        const trigger = container.querySelector('.delay-picker .trigger') as HTMLButtonElement;
        await fireEvent.click(trigger);
        await tick();
        const fifteenMin = container.querySelector(
            `.delay-picker [data-preset="${15 * 60}"]`,
        ) as HTMLButtonElement;
        await fireEvent.click(fifteenMin);
        await tick();

        const lock = container.querySelector('[data-command="lock"]') as HTMLElement;
        await fireEvent.click(lock); // arm
        await tick();
        await fireEvent.click(lock); // confirm
        await tick();
        await tick();
        const scheduleCall = mockCallSpy.mock.calls.find(
            ([method]) => method === 'action.schedule',
        );
        expect(scheduleCall).toBeDefined();
        const params = scheduleCall![1] as {
            delay_secs?: number;
            label?: string;
            action?: { action?: { data?: { payload?: { command?: string } } } };
        };
        expect(params.delay_secs).toBe(15 * 60);
        expect(params.label).toBe('Lock');
        expect(params.action?.action?.data?.payload?.command).toBe('lock');
        // No action.invoke call when delay is non-zero.
        const invokeCall = mockCallSpy.mock.calls.find(([method]) => method === 'action.invoke');
        expect(invokeCall).toBeUndefined();
    });

    it('backdrop click calls view.hide', async () => {
        const { container } = render(App);
        await tick();
        const backdrop = container.querySelector('.backdrop') as HTMLElement;
        await fireEvent.click(backdrop);
        await tick();
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'widgets/power-menu',
        );
        expect(hidden).toBe(true);
    });

    it('Escape key calls view.hide', async () => {
        render(App);
        await tick();
        await fireEvent.keyDown(document, { key: 'Escape' });
        await tick();
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'widgets/power-menu',
        );
        expect(hidden).toBe(true);
    });
});
