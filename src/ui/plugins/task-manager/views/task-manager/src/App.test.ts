import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import {
    PROCESSES_EVENT_CHANNEL,
    PROCESSES_WATCH,
    PROCESSES_UNWATCH,
    type Client,
} from '@quantum/client';
import type { ProcessSnapshot } from '@quantum/client';
import App from './App.svelte';

/** A minimal but well-formed snapshot the tree can render. */
function sampleSnapshot(): ProcessSnapshot {
    const leaf = (pid: number, name: string) => ({
        pid,
        name,
        cpu_percent: 1,
        mem_bytes: 100 * 1024 * 1024,
        aggregate_cpu_percent: 1,
        aggregate_mem_bytes: 100 * 1024 * 1024,
        protected: false,
        children: [],
    });
    return {
        global: {
            cpu_percent: 18,
            mem_used_bytes: 8 * 1024 * 1024 * 1024,
            mem_total_bytes: 32 * 1024 * 1024 * 1024,
            net_rx_bytes_per_second: 1024 * 1024,
            net_tx_bytes_per_second: 256 * 1024,
        },
        apps: [leaf(4001, 'firefox'), leaf(4002, 'kitty')],
        background: [leaf(4003, 'systemd')],
    };
}

/** A snapshot whose single app root carries a distinguishing name. */
function snapshotWithApp(name: string): ProcessSnapshot {
    const snapshot = sampleSnapshot();
    snapshot.apps = [
        {
            pid: 5000,
            name,
            cpu_percent: 1,
            mem_bytes: 100 * 1024 * 1024,
            aggregate_cpu_percent: 1,
            aggregate_mem_bytes: 100 * 1024 * 1024,
            protected: false,
            children: [],
        },
    ];
    snapshot.background = [];
    return snapshot;
}

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

    it('renders the process tree from the latest snapshot', async () => {
        const stub = createStubClient();
        const { getByText } = render(App, { props: { client: stub.client } });

        await vi.waitFor(() => expect(stub.subscribe).toHaveBeenCalled());

        stub.emit(sampleSnapshot());

        await vi.waitFor(() => {
            // The Apps section header and an app root are rendered by the tree.
            expect(getByText(/Apps/)).toBeTruthy();
            expect(getByText('firefox')).toBeTruthy();
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

    it('filters the tree to matching processes as the filter is typed', async () => {
        const stub = createStubClient();
        const { getByText, queryByText, getByLabelText } = render(App, {
            props: { client: stub.client },
        });

        await vi.waitFor(() => expect(stub.subscribe).toHaveBeenCalled());
        stub.emit(sampleSnapshot());
        await vi.waitFor(() => expect(getByText('firefox')).toBeTruthy());
        // Both apps are visible before filtering.
        expect(queryByText('kitty')).toBeTruthy();

        await fireEvent.input(getByLabelText('Filter processes'), {
            target: { value: 'firefox' },
        });

        await vi.waitFor(() => {
            expect(getByText('firefox')).toBeTruthy();
            // The non-matching app and background process drop out.
            expect(queryByText('kitty')).toBeNull();
            expect(queryByText('systemd')).toBeNull();
        });
    });

    it('shows the empty state when the filter matches nothing', async () => {
        const stub = createStubClient();
        const { getByText, getByLabelText } = render(App, { props: { client: stub.client } });

        await vi.waitFor(() => expect(stub.subscribe).toHaveBeenCalled());
        stub.emit(sampleSnapshot());
        await vi.waitFor(() => expect(getByText('firefox')).toBeTruthy());

        await fireEvent.input(getByLabelText('Filter processes'), {
            target: { value: 'zzz-nothing' },
        });

        await vi.waitFor(() =>
            expect(getByText('No processes match "zzz-nothing"')).toBeTruthy(),
        );
    });

    it('coalesces rapid snapshots into one applied render per animation frame (latest wins)', async () => {
        // Drive requestAnimationFrame manually so the coalescing is deterministic:
        // capture the scheduled callback instead of waiting on the real timer.
        const frames: FrameRequestCallback[] = [];
        const original = window.requestAnimationFrame;
        const rafSpy = vi.fn((callback: FrameRequestCallback) => {
            frames.push(callback);
            return frames.length;
        });
        window.requestAnimationFrame =
            rafSpy as unknown as typeof window.requestAnimationFrame;

        try {
            const stub = createStubClient();
            const { getByText, queryByText } = render(App, { props: { client: stub.client } });
            await vi.waitFor(() => expect(stub.subscribe).toHaveBeenCalled());

            // Three snapshots arrive back-to-back before a frame can fire.
            stub.emit(snapshotWithApp('proc-alpha'));
            stub.emit(snapshotWithApp('proc-beta'));
            stub.emit(snapshotWithApp('proc-gamma'));

            // Exactly one frame is scheduled for the whole burst, and nothing is
            // applied to the reactive snapshot until that frame fires.
            expect(rafSpy).toHaveBeenCalledTimes(1);
            expect(queryByText('proc-alpha')).toBeNull();
            expect(queryByText('proc-gamma')).toBeNull();

            // Fire the single scheduled frame; only the LAST payload is applied.
            frames[0](0);

            await vi.waitFor(() => {
                expect(getByText('proc-gamma')).toBeTruthy();
                expect(queryByText('proc-alpha')).toBeNull();
                expect(queryByText('proc-beta')).toBeNull();
            });
        } finally {
            window.requestAnimationFrame = original;
        }
    });

    it('clears a non-empty filter on Escape without closing the panel', async () => {
        const stub = createStubClient();
        const { getByText, queryByText, getByLabelText } = render(App, {
            props: { client: stub.client },
        });

        await vi.waitFor(() => expect(stub.subscribe).toHaveBeenCalled());
        stub.emit(sampleSnapshot());
        await vi.waitFor(() => expect(getByText('firefox')).toBeTruthy());

        await fireEvent.input(getByLabelText('Filter processes'), {
            target: { value: 'firefox' },
        });
        await vi.waitFor(() => expect(queryByText('kitty')).toBeNull());

        await fireEvent.keyDown(window, { key: 'Escape' });

        // The filter is cleared, so the pruned-out app returns, and the panel is
        // NOT hidden (Escape only clears while the filter is non-empty).
        await vi.waitFor(() => expect(queryByText('kitty')).toBeTruthy());
        expect(stub.call).not.toHaveBeenCalledWith('view.hide', {
            name: 'plugin/task-manager/task-manager',
        });
    });
});
