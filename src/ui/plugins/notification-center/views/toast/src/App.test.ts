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
let changeSubscribers: Array<(change: { type: string } | null) => void> = [];

function emit(list: PendingNotification[]): void {
    for (const callback of storeSubscribers) callback(list);
}

function emitChange(change: { type: string } | null): void {
    for (const callback of changeSubscribers) callback(change);
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
        subscribe: (
            callback: (list: PendingNotification[]) => void,
            onChange?: (change: { type: string } | null) => void,
        ) => {
            storeSubscribers.push(callback);
            if (onChange) changeSubscribers.push(onChange);
            return () => {
                storeSubscribers = storeSubscribers.filter((entry) => entry !== callback);
                if (onChange) {
                    changeSubscribers = changeSubscribers.filter((entry) => entry !== onChange);
                }
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
    changeSubscribers = [];
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

    it('auto-dismisses with the default duration when timeout_ms is 0', async () => {
        // timeout_ms 0 ("never expire") keeps the notification in the center,
        // but the transient toast popup still auto-dismisses on the default.
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

    it('floors a very short timeout to the minimum visible duration', async () => {
        const { container } = render(App);
        await tick();
        emit([makeNotification({ id: 3, timeout_ms: 800 })]);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);

        // Still visible past its requested 800ms because of the 3000ms floor.
        vi.advanceTimersByTime(900);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);

        // Gone once the floor elapses.
        vi.advanceTimersByTime(2101);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(0);
    });

    it('auto-dismisses a critical notification (it persists in the center, not on screen)', async () => {
        const { container } = render(App);
        await tick();
        emit([makeNotification({ id: 4, urgency: 'critical', timeout_ms: 5000 })]);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);

        vi.advanceTimersByTime(5001);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(0);
    });

    it('clears all visible toasts on a toasts_cleared change', async () => {
        const { container } = render(App);
        await tick();
        emit([
            makeNotification({ id: 1, summary: 'One' }),
            makeNotification({ id: 2, summary: 'Two' }),
        ]);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(2);

        // The user opened the notification center: clear the transient toasts
        // and hide the surface.
        emitChange({ type: 'toasts_cleared' });
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(0);
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/notification-center/toast',
        );
        expect(hidden).toBe(true);
    });

    it('pauses the auto-dismiss while hovered and resumes on mouse leave', async () => {
        const { container } = render(App);
        await tick();
        emit([makeNotification({ id: 7, timeout_ms: 5000 })]);
        await tick();
        const toast = container.querySelector('.toast') as HTMLElement;
        expect(toast).not.toBeNull();

        // Hovering pauses the timer: advancing past the lifetime must not dismiss.
        await fireEvent.mouseEnter(toast);
        vi.advanceTimersByTime(6000);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);

        // Leaving resumes with the banked remaining time.
        await fireEvent.mouseLeave(toast);
        vi.advanceTimersByTime(5001);
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

    it('hides_when_shown_but_no_notifications_arrive', async () => {
        // The daemon maps the overlay surface on a `created` event, but the
        // store may deliver nothing (immediately-closed/replaced notification,
        // zero-timeout edge case, or a show->subscribe race). The debounced
        // guard must unmap the empty window so it stops capturing pointer input.
        render(App);
        await tick();

        const hiddenBefore = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/notification-center/toast',
        );
        expect(hiddenBefore).toBe(false);

        // Advance past the debounce window.
        vi.advanceTimersByTime(601);
        await tick();

        const hiddenAfter = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/notification-center/toast',
        );
        expect(hiddenAfter).toBe(true);
    });

    it('does_not_hide_if_a_notification_arrives_before_debounce', async () => {
        // A real notification can arrive a few hundred milliseconds after the
        // daemon shows the window (the show->subscribe race). The debounce must
        // be long enough that the card renders first and the window is NOT hidden.
        const { container } = render(App);
        await tick();

        // A card arrives before the debounce elapses.
        vi.advanceTimersByTime(300);
        emit([makeNotification({ id: 42, timeout_ms: 5000 })]);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);

        // Advance past where the debounce would have fired, but stay well short
        // of the 5000ms auto-dismiss so the card remains visible.
        vi.advanceTimersByTime(1000);
        await tick();

        expect(container.querySelectorAll('.toast')).toHaveLength(1);
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/notification-center/toast',
        );
        expect(hidden).toBe(false);
    });
});
