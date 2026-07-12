import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import {
    PROCESSES_EVENT_CHANNEL,
    PROCESSES_WATCH,
    PROCESSES_UNWATCH,
    type Client,
} from '@quantum/client';
import App from './App.svelte';

/**
 * A hand-rolled stub `Client` with spies on every method. `subscribe` records
 * the channel and callback so a test can push a synthetic snapshot, and returns
 * a spy unsubscribe so cleanup assertions are possible.
 */
function createStubClient(): {
    client: Client;
    call: ReturnType<typeof vi.fn>;
    subscribe: ReturnType<typeof vi.fn>;
    unsubscribe: ReturnType<typeof vi.fn>;
    emit: (payload: unknown) => void;
} {
    const unsubscribe = vi.fn();
    let handler: ((payload: unknown) => void) | null = null;
    const call = vi.fn(() => Promise.resolve(null));
    const subscribe = vi.fn((_channel: string, callback: (payload: unknown) => void) => {
        handler = callback;
        return unsubscribe;
    });
    const client: Client = {
        call: call as unknown as Client['call'],
        subscribe: subscribe as unknown as Client['subscribe'],
        close: vi.fn(),
    };
    return {
        client,
        call,
        subscribe,
        unsubscribe,
        emit: (payload: unknown) => handler?.(payload),
    };
}

// Bound queries default to `document.body`, and each `render` appends there;
// without cleanup, prior tests' DOM (and their document keydown listeners)
// accumulate and collide with the current render.
afterEach(cleanup);

describe('App shell', () => {
    it('renders the titlebar title and a close button', () => {
        const { client } = createStubClient();
        const { getByText, getByRole } = render(App, { props: { client } });

        expect(getByText('Task Manager')).toBeTruthy();
        expect(getByRole('button', { name: 'Close' })).toBeTruthy();
    });

    it('starts the process watch and subscribes to the snapshot channel on mount', async () => {
        const stub = createStubClient();
        render(App, { props: { client: stub.client } });

        await vi.waitFor(() => {
            expect(stub.call).toHaveBeenCalledWith(PROCESSES_WATCH, {});
            expect(stub.subscribe).toHaveBeenCalledWith(
                PROCESSES_EVENT_CHANNEL,
                expect.any(Function),
            );
        });
    });

    it('reflects the latest snapshot in the placeholder body', async () => {
        const stub = createStubClient();
        const { getByText } = render(App, { props: { client: stub.client } });

        await vi.waitFor(() => expect(stub.subscribe).toHaveBeenCalled());

        stub.emit({
            global: {
                cpu_percent: 0,
                mem_used_bytes: 0,
                mem_total_bytes: 0,
                net_rx_bytes_per_second: 0,
                net_tx_bytes_per_second: 0,
            },
            apps: [{ name: 'firefox' }, { name: 'kitty' }],
            background: [{ name: 'systemd' }],
        });

        await vi.waitFor(() => {
            expect(getByText('3 top-level processes')).toBeTruthy();
        });
    });

    it('stops the watch and hides the view when the close button is clicked', async () => {
        const stub = createStubClient();
        const { getByRole } = render(App, { props: { client: stub.client } });

        await fireEvent.click(getByRole('button', { name: 'Close' }));

        expect(stub.call).toHaveBeenCalledWith(PROCESSES_UNWATCH, {});
        expect(stub.call).toHaveBeenCalledWith('view.hide', {
            name: 'plugin/task-manager/task-manager',
        });
    });

    it('closes on Escape when the filter is empty', async () => {
        const stub = createStubClient();
        render(App, { props: { client: stub.client } });

        // The document keydown listener is registered inside an `$effect`, which
        // flushes on a microtask after mount; wait for the mount effects to run
        // before dispatching so the listener is attached.
        await vi.waitFor(() => expect(stub.subscribe).toHaveBeenCalled());

        await fireEvent.keyDown(window, { key: 'Escape' });

        expect(stub.call).toHaveBeenCalledWith('view.hide', {
            name: 'plugin/task-manager/task-manager',
        });
    });
});
