import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import type { PendingNotification } from '@quantum/client';

/*
 * Module-level mock state. `beforeEach` resets these so the `vi.mock`
 * factory below (which closes over them by reference) sees fresh values
 * per test. `storeSubscribers` collects the callbacks registered through
 * `createNotificationStore(client).subscribe(...)` so a test can drive
 * the store by emitting a fresh notification snapshot to all of them.
 */
let mockCallSpy = vi.fn();
let storeSubscribers: Array<(list: PendingNotification[]) => void> = [];

function emit(list: PendingNotification[]): void {
    for (const callback of storeSubscribers) callback(list);
}

vi.mock('@quantum/client', () => ({
    createClient: () => ({
        call: (...args: unknown[]) => {
            mockCallSpy(...args);
            return Promise.resolve(undefined);
        },
        subscribe: () => () => {},
        close: vi.fn(),
    }),
    createNotificationStore: () => ({
        subscribe: (callback: (list: PendingNotification[]) => void) => {
            storeSubscribers.push(callback);
            return () => {
                storeSubscribers = storeSubscribers.filter((entry) => entry !== callback);
            };
        },
    }),
    __esModule: true,
}));

import App from './App.svelte';

function makeNotification(overrides: Partial<PendingNotification> = {}): PendingNotification {
    return {
        id: 1,
        app_name: 'Spotify',
        summary: 'Now playing',
        body: 'Some song',
        icon: 'spotify',
        urgency: 'normal',
        timeout_ms: 5000,
        actions: [],
        ...overrides,
    };
}

beforeEach(() => {
    mockCallSpy = vi.fn();
    storeSubscribers = [];
    vi.useFakeTimers();
});

afterEach(() => {
    vi.useRealTimers();
});

describe('NotificationToast App', () => {
    it('renders a toast card after the store emits one notification', async () => {
        const { container } = render(App);
        await tick();
        emit([makeNotification({ id: 1, app_name: 'Spotify', summary: 'Now playing' })]);
        await tick();
        const cards = container.querySelectorAll('.toast');
        expect(cards).toHaveLength(1);
        expect(container.textContent).toContain('Spotify');
        expect(container.textContent).toContain('Now playing');
    });

    it('auto-dismisses after timeout_ms and hides the view when empty', async () => {
        const { container } = render(App);
        await tick();
        emit([makeNotification({ id: 1, timeout_ms: 5000 })]);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);

        vi.advanceTimersByTime(5001);
        await tick();

        expect(container.querySelectorAll('.toast')).toHaveLength(0);
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/notification-center/toast',
        );
        expect(hidden).toBe(true);
    });

    it('uses the 5000ms default when timeout_ms is 0', async () => {
        const { container } = render(App);
        await tick();
        emit([makeNotification({ id: 2, timeout_ms: 0 })]);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);

        vi.advanceTimersByTime(4999);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);

        vi.advanceTimersByTime(2);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(0);
    });

    it('clicking a toast calls action.invoke with a dismiss envelope for that id', async () => {
        const { container } = render(App);
        await tick();
        emit([makeNotification({ id: 9, app_name: 'Slack' })]);
        await tick();
        const toast = container.querySelector('.toast') as HTMLElement;
        expect(toast).not.toBeNull();
        await fireEvent.click(toast);
        await tick();
        const invokeCall = mockCallSpy.mock.calls.find(([method]) => method === 'action.invoke');
        expect(invokeCall).toBeDefined();
        const params = invokeCall![1] as {
            provider?: string;
            action?: { kind?: string; data?: { kind?: string; payload?: unknown } };
        };
        expect(params.provider).toBe('notifications');
        expect(params.action?.kind).toBe('custom');
        expect(params.action?.data?.kind).toBe('notifications');
        expect(params.action?.data?.payload).toEqual({ command: 'dismiss', id: 9 });
    });

    it('clicking a toast removes it from the visible set', async () => {
        const { container } = render(App);
        await tick();
        emit([makeNotification({ id: 9 })]);
        await tick();
        const toast = container.querySelector('.toast') as HTMLElement;
        await fireEvent.click(toast);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(0);
    });
});
