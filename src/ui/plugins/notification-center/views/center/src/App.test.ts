import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import { closeContextMenu, type PendingNotification } from '@quantum/client';

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

// Spread the actual module so the real context-menu runtime
// (openContextMenu/closeContextMenu) stays intact; only the client and store
// factories are stubbed.
vi.mock('@quantum/client', async (importActual) => {
    const actual = await importActual<typeof import('@quantum/client')>();
    return {
        ...actual,
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
    };
});

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
});

afterEach(() => {
    closeContextMenu();
});

describe('NotificationCenter App', () => {
    it('renders the backdrop and a No notifications empty state initially', async () => {
        const { container } = render(App);
        await tick();
        expect(container.querySelector('.backdrop')).not.toBeNull();
        expect(container.querySelector('.panel')).not.toBeNull();
        expect(container.querySelector('.empty')).not.toBeNull();
        expect(container.textContent).toContain('No notifications');
    });

    it('renders a card per notification once the store emits', async () => {
        const { container } = render(App);
        await tick();
        emit([
            makeNotification({ id: 1, app_name: 'Spotify', summary: 'Now playing' }),
            makeNotification({ id: 2, app_name: 'Slack', summary: 'New message' }),
        ]);
        await tick();
        const cards = container.querySelectorAll('.card');
        expect(cards).toHaveLength(2);
        expect(container.textContent).toContain('Spotify');
        expect(container.textContent).toContain('Now playing');
        expect(container.textContent).toContain('Slack');
        expect(container.textContent).toContain('New message');
        expect(container.querySelector('.empty')).toBeNull();
    });

    it('dismiss button calls action.invoke with a dismiss envelope for the right id', async () => {
        const { container } = render(App);
        await tick();
        emit([
            makeNotification({ id: 7, app_name: 'Spotify' }),
            makeNotification({ id: 9, app_name: 'Slack' }),
        ]);
        await tick();
        const dismissButtons = container.querySelectorAll('.dismiss');
        expect(dismissButtons).toHaveLength(2);
        await fireEvent.click(dismissButtons[1] as HTMLElement);
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

    it('Dismiss all dismisses every notification', async () => {
        const { container } = render(App);
        await tick();
        emit([
            makeNotification({ id: 7, app_name: 'Spotify' }),
            makeNotification({ id: 9, app_name: 'Slack' }),
        ]);
        await tick();
        const dismissAll = container.querySelector('.dismiss-all') as HTMLElement;
        expect(dismissAll).not.toBeNull();
        await fireEvent.click(dismissAll);
        await tick();
        const dismissed = mockCallSpy.mock.calls
            .filter(([method]) => method === 'action.invoke')
            .map(
                ([, params]) =>
                    (
                        params as {
                            action?: { data?: { payload?: { command?: string; id?: number } } };
                        }
                    ).action?.data?.payload,
            )
            .filter((payload) => payload?.command === 'dismiss')
            .map((payload) => payload?.id);
        expect(dismissed).toContain(7);
        expect(dismissed).toContain(9);
    });

    it('hides the Dismiss all button when there are no notifications', async () => {
        const { container } = render(App);
        await tick();
        expect(container.querySelector('.dismiss-all')).toBeNull();
    });

    it('action button calls action.invoke with an action envelope carrying the action key', async () => {
        const { container } = render(App);
        await tick();
        emit([
            makeNotification({ id: 3, app_name: 'Slack', actions: [['reply', 'Reply']] }),
        ]);
        await tick();
        const actionButton = container.querySelector('.action') as HTMLElement;
        expect(actionButton).not.toBeNull();
        expect(actionButton.textContent).toContain('Reply');
        await fireEvent.click(actionButton);
        await tick();
        const invokeCall = mockCallSpy.mock.calls.find(([method]) => method === 'action.invoke');
        expect(invokeCall).toBeDefined();
        const params = invokeCall![1] as {
            action?: { data?: { payload?: unknown } };
        };
        expect(params.action?.data?.payload).toEqual({
            command: 'action',
            id: 3,
            action_key: 'reply',
        });
    });

    it('right-click on a card opens a menu whose Dismiss calls action.invoke', async () => {
        const { container } = render(App);
        await tick();
        emit([makeNotification({ id: 5, app_name: 'Spotify' })]);
        await tick();

        expect(document.querySelector('[data-quantum-context-menu]')).toBeNull();
        await fireEvent.contextMenu(container.querySelector('.card') as HTMLElement);
        await tick();

        const menu = document.querySelector('[data-quantum-context-menu]');
        expect(menu).not.toBeNull();
        const dismiss = Array.from(menu!.querySelectorAll('[role="menuitem"]')).find((el) =>
            el.textContent?.includes('Dismiss'),
        ) as HTMLButtonElement;
        expect(dismiss).toBeTruthy();
        await fireEvent.click(dismiss);
        await tick();

        const invokeCall = mockCallSpy.mock.calls.find(
            ([method, params]) =>
                method === 'action.invoke' &&
                (params as { action?: { data?: { payload?: { command?: string } } } }).action?.data
                    ?.payload?.command === 'dismiss',
        );
        expect(invokeCall).toBeDefined();
        const params = invokeCall![1] as {
            action?: { data?: { payload?: { command?: string; id?: number } } };
        };
        expect(params.action?.data?.payload).toEqual({ command: 'dismiss', id: 5 });
    });

    it('backdrop click calls view.hide with the base center name', async () => {
        const { container } = render(App);
        await tick();
        const backdrop = container.querySelector('.backdrop') as HTMLElement;
        await fireEvent.click(backdrop);
        await tick();
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/notification-center/center',
        );
        expect(hidden).toBe(true);
    });

    it('Escape key calls view.hide with the base center name', async () => {
        render(App);
        await tick();
        await fireEvent.keyDown(document, { key: 'Escape' });
        await tick();
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/notification-center/center',
        );
        expect(hidden).toBe(true);
    });
});
