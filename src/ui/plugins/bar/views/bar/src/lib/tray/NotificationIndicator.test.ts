import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import NotificationIndicator from './NotificationIndicator.svelte';

/**
 * Build a hand-rolled mock client that captures the subscription
 * callback registered for the `notifications.event` channel so the
 * test can drive notification snapshots directly, mirroring the
 * pattern used by MediaControls.test.ts.
 */
function makeMockClient() {
    let savedCallback: ((payload: unknown) => void) | undefined;
    const call = vi.fn().mockResolvedValue(undefined);
    const subscribe = vi.fn((channel: string, cb: (payload: unknown) => void) => {
        if (channel === 'notifications.event') {
            savedCallback = cb;
        }
        return () => {};
    });
    const client = { call, subscribe, close: vi.fn() };
    return {
        client,
        call,
        emit(notifications: unknown[]) {
            savedCallback?.({
                change: { type: 'created', data: { id: 1, timeout_ms: 5000 } },
                notifications,
            });
        },
    };
}

afterEach(() => {
    (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = undefined;
});

describe('NotificationIndicator', () => {
    it('shows no badge initially and a badge of "2" after two notifications arrive', async () => {
        const { client, emit } = makeMockClient();
        const { container } = render(NotificationIndicator, {
            props: { client: client as never },
        });
        await tick();

        expect(container.querySelector('.notification-badge')).toBeNull();

        emit([{ id: 1 }, { id: 2 }]);
        await tick();

        const badge = container.querySelector('.notification-badge');
        expect(badge).not.toBeNull();
        expect(badge?.textContent).toBe('2');
    });

    it('toggles the notification center on click', async () => {
        const { client, call } = makeMockClient();
        const { container } = render(NotificationIndicator, {
            props: { client: client as never },
        });
        await tick();

        const button = container.querySelector('button');
        expect(button).not.toBeNull();
        await fireEvent.click(button as HTMLButtonElement);

        expect(call).toHaveBeenCalledWith(
            'view.toggle',
            expect.objectContaining({
                name: expect.stringContaining('plugin/notification-center/center'),
            }),
        );
    });

    it('appends the @monitor suffix to the view name when set', async () => {
        (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = 'DP-1';
        const { client, call } = makeMockClient();
        const { container } = render(NotificationIndicator, {
            props: { client: client as never },
        });
        await tick();

        await fireEvent.click(container.querySelector('button') as HTMLButtonElement);

        expect(call).toHaveBeenCalledWith('view.toggle', {
            name: 'plugin/notification-center/center@DP-1',
        });
    });
});
