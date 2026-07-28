import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import { closeContextMenu } from '@quantum/client';
import TimerIndicator from './TimerIndicator.svelte';

function contextMenu(): HTMLElement | null {
    return document.querySelector('[data-quantum-context-menu]');
}

function menuItem(text: string): HTMLButtonElement | undefined {
    return Array.from(
        document.querySelectorAll('[data-quantum-context-menu] [role="menuitem"]'),
    ).find((el) => el.textContent?.includes(text)) as HTMLButtonElement | undefined;
}

/**
 * Minimal timer fixtures. The ring reads `status`, `id`, and
 * `kind.end_unix`, so those fields are provided and the rest of the
 * timer shape is cast at the mock boundary. `firesAtUnix` defaults to
 * the far future so an active timer is not treated as fired; pass a past
 * value (for example `1`) to simulate a fired timer.
 */
function timer(status: 'active' | 'expired', firesAtUnix = 4102444800): unknown {
    return {
        id: Math.random().toString(16).slice(2),
        status,
        kind: { type: 'one_shot', end_unix: firesAtUnix },
    };
}

function snapshot(timers: unknown[]): unknown {
    return { settings: {}, timers };
}

/**
 * Build a hand-rolled mock client. `createTimerStore` calls
 * `client.call('timer.list', {})` then subscribes to the
 * `timer.event` channel, so the mock captures that callback to let
 * tests drive snapshots directly. `call` is configurable per test so
 * `timer.list` can resolve a seeded snapshot.
 */
function makeMockClient(listSnapshot?: unknown) {
    let savedCallback: ((payload: unknown) => void) | undefined;
    const call = vi.fn((method: string) => {
        if (method === 'timer.list') {
            return Promise.resolve(listSnapshot);
        }
        return Promise.resolve(undefined);
    });
    const subscribe = vi.fn((channel: string, cb: (payload: unknown) => void) => {
        if (channel === 'timer.event') {
            savedCallback = cb;
        }
        return () => {};
    });
    const client = { call, subscribe, close: vi.fn() };
    return {
        client,
        call,
        emit(payload: unknown) {
            savedCallback?.(payload);
        },
    };
}

afterEach(() => {
    closeContextMenu();
    (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = undefined;
});

describe('TimerIndicator', () => {
    it('toggles the timer-create overlay on click', async () => {
        const { client, call } = makeMockClient();
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();

        const button = container.querySelector('button');
        expect(button).not.toBeNull();
        await fireEvent.click(button as HTMLButtonElement);

        expect(call).toHaveBeenCalledWith('view.toggle', {
            name: 'plugin/timer-create/timer-create',
        });
    });

    it('appends the @monitor suffix to the view name when set', async () => {
        (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = 'DP-1';
        const { client, call } = makeMockClient();
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();

        await fireEvent.click(container.querySelector('button') as HTMLButtonElement);

        expect(call).toHaveBeenCalledWith('view.toggle', {
            name: 'plugin/timer-create/timer-create@DP-1',
        });
    });

    it('shows the stopwatch and no ring initially', async () => {
        const { client } = makeMockClient(snapshot([]));
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();

        expect(container.querySelector('[data-testid="timer-ring"]')).toBeNull();
        expect(container.querySelector('.icon-box svg')).not.toBeNull();
    });

    it('shows the ring and no badge when active timers exist in the snapshot', async () => {
        const { client } = makeMockClient(
            snapshot([
                timer('active'),
                timer('active'),
                timer('expired'),
            ]),
        );
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();
        await tick();

        expect(container.querySelector('[data-testid="timer-ring"]')).not.toBeNull();
        expect(container.querySelector('.timer-badge')).toBeNull();
    });

    it('drops the ring and shows the stopwatch when a snapshot goes to zero active', async () => {
        const { client, emit } = makeMockClient(snapshot([timer('active')]));
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();
        await tick();

        expect(container.querySelector('[data-testid="timer-ring"]')).not.toBeNull();

        emit({
            change: 'dismissed',
            ...snapshot([timer('expired')]),
        });
        await tick();

        expect(container.querySelector('[data-testid="timer-ring"]')).toBeNull();
        expect(container.querySelector('.icon-box svg')).not.toBeNull();
    });

    it('marks the ring as fired when the soonest active timer has already elapsed', async () => {
        const { client } = makeMockClient(snapshot([timer('active', 1)]));
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();
        await tick();

        const ring = container.querySelector('[data-testid="timer-ring"]');
        expect(ring).not.toBeNull();
        expect(ring?.getAttribute('data-fired')).toBe('true');
    });

    it('opens a quick-actions menu on right-click', async () => {
        const { client } = makeMockClient();
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();

        expect(contextMenu()).toBeNull();

        const button = container.querySelector('button') as HTMLButtonElement;
        await fireEvent.contextMenu(button);
        await tick();

        expect(contextMenu()).not.toBeNull();
    });

    it('dismisses all timers from the menu and closes it', async () => {
        const { client, call } = makeMockClient();
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();

        await fireEvent.contextMenu(container.querySelector('button') as HTMLButtonElement);
        await tick();

        const dismiss = menuItem('Dismiss all');
        expect(dismiss).toBeTruthy();
        await fireEvent.click(dismiss as HTMLButtonElement);
        await tick();

        expect(call).toHaveBeenCalledWith('timer.dismiss_all', {});
        expect(contextMenu()).toBeNull();
    });

    it('opens the creation overlay from the menu and closes it', async () => {
        const { client, call } = makeMockClient();
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();

        await fireEvent.contextMenu(container.querySelector('button') as HTMLButtonElement);
        await tick();

        const open = menuItem('Open timers');
        expect(open).toBeTruthy();
        await fireEvent.click(open as HTMLButtonElement);
        await tick();

        expect(call).toHaveBeenCalledWith('view.toggle', {
            name: 'plugin/timer-create/timer-create',
        });
        expect(contextMenu()).toBeNull();
    });

    it('expands the input region on open and resets it on close', async () => {
        const { client, call } = makeMockClient();
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();

        await fireEvent.contextMenu(container.querySelector('button') as HTMLButtonElement);
        await tick();

        const openCall = call.mock.calls.find(
            (args) =>
                args[0] === 'view.set_input_region' &&
                (args[1] as { region: unknown }).region !== null,
        );
        expect(openCall).toBeTruthy();
        const region = (openCall![1] as { region: { x: number; y: number; width: number; height: number } }).region;
        expect(typeof region.x).toBe('number');
        expect(typeof region.y).toBe('number');
        expect(typeof region.width).toBe('number');
        expect(typeof region.height).toBe('number');

        call.mockClear();
        closeContextMenu();
        await tick();

        expect(call).toHaveBeenCalledWith('view.set_input_region', {
            name: 'plugin/bar/bar',
            region: null,
        });
    });

    it('closes the menu on Escape', async () => {
        const { client } = makeMockClient();
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();

        await fireEvent.contextMenu(container.querySelector('button') as HTMLButtonElement);
        await tick();
        expect(contextMenu()).not.toBeNull();

        await fireEvent.keyDown(window, { key: 'Escape' });
        await tick();
        expect(contextMenu()).toBeNull();
    });
});
