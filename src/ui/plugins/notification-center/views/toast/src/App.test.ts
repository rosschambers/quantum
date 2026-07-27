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

function emitChange(change: { type: string } | null): void {
    for (const callback of changeSubscribers) callback(change);
}

// Emit a full envelope the way the real store does: the list callback fires
// first, then the change callback, for the same delivery. `change` is null for
// the initial catch-up snapshot and a descriptor for every live event.
function emitEnvelope(list: PendingNotification[], change: { type: string } | null): void {
    for (const callback of storeSubscribers) callback(list);
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
        emitEnvelope([makeNotification({ id: 1, app_name: 'Spotify', summary: 'Now playing' })], {
            type: 'created',
        });
        await tick();
        const cards = container.querySelectorAll('.toast');
        expect(cards).toHaveLength(1);
        expect(container.textContent).toContain('Spotify');
        expect(container.textContent).toContain('Now playing');
    });

    it('auto-dismisses after timeout_ms and hides the view when empty', async () => {
        const { container } = render(App);
        await tick();
        emitEnvelope([makeNotification({ id: 1, timeout_ms: 5000 })], { type: 'created' });
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
        emitEnvelope([makeNotification({ id: 2, timeout_ms: 0 })], { type: 'created' });
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
        emitEnvelope([makeNotification({ id: 3, timeout_ms: 800 })], { type: 'created' });
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
        emitEnvelope([makeNotification({ id: 4, urgency: 'critical', timeout_ms: 5000 })], {
            type: 'created',
        });
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);

        vi.advanceTimersByTime(5001);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(0);
    });

    it('only toasts the newest notification on a catch-up snapshot, not the whole backlog', async () => {
        // A rebuilt toast window (monitor change destroys and reconstructs the
        // single-instance surface) starts with an empty `seen` set and catches
        // up via `provider.query`, which returns the FULL pending set of every
        // active (undismissed) notification with a null change. Only the newest
        // notification (highest id) triggered the show and may pop; the backlog
        // of earlier notifications must NOT re-toast.
        const { container } = render(App);
        await tick();

        emitEnvelope(
            [
                makeNotification({ id: 3, summary: 'Newest', timeout_ms: 5000 }),
                makeNotification({ id: 2, summary: 'Middle', timeout_ms: 5000 }),
                makeNotification({ id: 1, summary: 'Oldest', timeout_ms: 5000 }),
            ],
            null,
        );
        await tick();

        expect(container.querySelectorAll('.toast')).toHaveLength(1);
        expect(container.textContent).toContain('Newest');
        expect(container.textContent).not.toContain('Middle');
        expect(container.textContent).not.toContain('Oldest');
    });

    it('does not re-toast a catch-up backlog after a timed-out toast and a fresh notification', async () => {
        // Reproduces the reported bug end to end. A notification times out (it
        // stays in the center), then a new one arrives on a monitor that rebuilt
        // the toast window: the fresh component's catch-up returns [B, A]. Only
        // B must pop.
        const { container } = render(App);
        await tick();

        // First notification pops as a live created event, then times out.
        emitEnvelope([makeNotification({ id: 1, summary: 'One', timeout_ms: 5000 })], {
            type: 'created',
        });
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(1);
        vi.advanceTimersByTime(5001);
        await tick();
        expect(container.querySelectorAll('.toast')).toHaveLength(0);

        // The window is rebuilt for a new notification: a fresh component would
        // catch up with the full pending set [B, A]. Model that catch-up here.
        emitEnvelope(
            [
                makeNotification({ id: 2, summary: 'Two', timeout_ms: 5000 }),
                makeNotification({ id: 1, summary: 'One', timeout_ms: 5000 }),
            ],
            null,
        );
        await tick();

        expect(container.querySelectorAll('.toast')).toHaveLength(1);
        expect(container.textContent).toContain('Two');
        expect(container.textContent).not.toContain('One');
    });

    it('clears all visible toasts on a toasts_cleared change', async () => {
        const { container } = render(App);
        await tick();
        emitEnvelope(
            [makeNotification({ id: 1, summary: 'One' }), makeNotification({ id: 2, summary: 'Two' })],
            { type: 'created' },
        );
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
        emitEnvelope([makeNotification({ id: 7, timeout_ms: 5000 })], { type: 'created' });
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
        emitEnvelope([makeNotification({ id: 9, app_name: 'Slack' })], { type: 'created' });
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
        emitEnvelope([makeNotification({ id: 9 })], { type: 'created' });
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
        emitEnvelope([makeNotification({ id: 42, timeout_ms: 5000 })], { type: 'created' });
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
