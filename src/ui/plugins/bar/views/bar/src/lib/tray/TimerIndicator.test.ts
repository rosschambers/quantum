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
 * Minimal timer fixtures. The badge only reads `status`, so the other
 * fields are omitted and the snapshot is cast at the mock boundary.
 */
function timer(status: 'active' | 'expired'): unknown {
    return { id: Math.random().toString(16).slice(2), status };
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

    it('shows no badge initially', async () => {
        const { client } = makeMockClient(snapshot([]));
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();

        expect(container.querySelector('.timer-badge')).toBeNull();
    });

    it('counts only active timers from the initial timer.list snapshot', async () => {
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

        const badge = container.querySelector('.timer-badge');
        expect(badge).not.toBeNull();
        expect(badge?.textContent).toBe('2');
    });

    it('updates the badge from a subsequent timer.event snapshot', async () => {
        const { client, emit } = makeMockClient(snapshot([timer('active')]));
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();
        await tick();

        emit({
            change: 'created',
            ...snapshot([timer('active'), timer('active'), timer('active')]),
        });
        await tick();

        const badge = container.querySelector('.timer-badge');
        expect(badge).not.toBeNull();
        expect(badge?.textContent).toBe('3');
    });

    it('caps the badge at "9+" for more than nine active timers', async () => {
        const many = Array.from({ length: 12 }, () => timer('active'));
        const { client } = makeMockClient(snapshot(many));
        const { container } = render(TimerIndicator, {
            props: { client: client as never },
        });
        await tick();
        await tick();

        expect(container.querySelector('.timer-badge')?.textContent).toBe('9+');
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
