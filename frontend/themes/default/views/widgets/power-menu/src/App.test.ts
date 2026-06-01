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
            if (method === 'provider.query' && (params as any)?.id === 'system_power') {
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

describe('PowerMenu App', () => {
    it('renders the backdrop and the centered card', () => {
        const { container } = render(App);
        expect(container.querySelector('.backdrop')).not.toBeNull();
        expect(container.querySelector('.card')).not.toBeNull();
    });

    it('queries the system_power provider on mount', async () => {
        render(App);
        await tick();
        await tick();
        const queried = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'provider.query' && (params as any)?.id === 'system_power',
        );
        expect(queried).toBe(true);
    });

    it('subscribes to system_power.event on mount', async () => {
        render(App);
        await tick();
        await tick();
        const subscribed = mockSubscribeSpy.mock.calls.some(
            ([channel]) => channel === 'system_power.event',
        );
        expect(subscribed).toBe(true);
    });

    it('renders an action row only for each capability that is true', async () => {
        const { container } = render(App);
        await new Promise((r) => setTimeout(r, 10));
        await tick();
        await tick();
        const rows = Array.from(container.querySelectorAll('.action-row')).map(
            (r) => r.textContent?.trim() ?? '',
        );
        // Each row's textContent is the label (e.g. "Suspend") when not armed.
        const labels = rows.join(' ');
        expect(labels).toContain('Shutdown');
        expect(labels).toContain('Restart');
        expect(labels).toContain('Suspend');
        expect(labels).toContain('Lock');
        // can_hibernate is false in the mocked capabilities.
        expect(labels).not.toContain('Hibernate');
    });

    it('fetches the scheduled jobs list on mount', async () => {
        render(App);
        await new Promise((r) => setTimeout(r, 10));
        await tick();
        const queried = mockCallSpy.mock.calls.some(([method]) => method === 'action.scheduled');
        expect(queried).toBe(true);
    });

    it('backdrop click calls view.hide', async () => {
        const { container } = render(App);
        await tick();
        const backdrop = container.querySelector('.backdrop') as HTMLElement;
        await fireEvent.click(backdrop);
        await tick();
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' && (params as any)?.name === 'widgets/power-menu',
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
                method === 'view.hide' && (params as any)?.name === 'widgets/power-menu',
        );
        expect(hidden).toBe(true);
    });

    it('clicking the close button calls view.hide', async () => {
        const { container } = render(App);
        await tick();
        const close = container.querySelector('.close') as HTMLElement;
        await fireEvent.click(close);
        await tick();
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' && (params as any)?.name === 'widgets/power-menu',
        );
        expect(hidden).toBe(true);
    });
});
