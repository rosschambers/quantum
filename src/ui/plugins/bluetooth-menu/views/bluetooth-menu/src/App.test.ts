import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import type { BluetoothState } from './lib/types';

let mockState: BluetoothState;
let mockCallSpy = vi.fn();
let capturedSubscribeCallback: ((payload: unknown) => void) | null = null;
export const openContextMenuSpy = vi.fn();

function baseState(): BluetoothState {
    return {
        available: true,
        powered: true,
        discovering: false,
        adapter_path: '/org/bluez/hci0',
        devices: [
            {
                address: 'AA:00:00:00:00:01',
                name: 'Speaker',
                battery_percent: 80,
                paired: true,
                trusted: true,
                connected: true,
                icon: 'audio-headset',
                rssi: null,
            },
            {
                address: 'AA:00:00:00:00:02',
                name: 'Keyboard',
                battery_percent: null,
                paired: true,
                trusted: false,
                connected: false,
                icon: 'input-keyboard',
                rssi: null,
            },
            {
                address: 'AA:00:00:00:00:03',
                name: 'Television',
                battery_percent: null,
                paired: false,
                trusted: false,
                connected: false,
                icon: null,
                rssi: -70,
            },
            {
                address: 'AA:00:00:00:00:04',
                name: 'Earbuds',
                battery_percent: null,
                paired: false,
                trusted: false,
                connected: false,
                icon: 'audio-headset',
                rssi: -40,
            },
        ],
    };
}

vi.mock('@quantum/client', () => ({
    createClient: () => ({
        call: (...args: unknown[]) => {
            mockCallSpy(...args);
            const [method, params] = args as [string, unknown];
            if (method === 'provider.query' && (params as { id?: string })?.id === 'bluetooth') {
                return Promise.resolve(mockState);
            }
            return Promise.resolve(undefined);
        },
        subscribe: (...args: unknown[]) => {
            const [, callback] = args as [string, (payload: unknown) => void];
            capturedSubscribeCallback = callback;
            return () => {};
        },
        close: vi.fn(),
    }),
    openContextMenu: (...args: unknown[]) => openContextMenuSpy(...args),
    closeContextMenu: vi.fn(),
    __esModule: true,
}));

import App from './App.svelte';

beforeEach(() => {
    mockCallSpy = vi.fn();
    openContextMenuSpy.mockClear();
    capturedSubscribeCallback = null;
    mockState = baseState();
});

async function settle(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 10));
    await tick();
    await tick();
}

function invokePayloads(): Array<Record<string, unknown>> {
    return mockCallSpy.mock.calls
        .filter(([method]) => method === 'action.invoke')
        .map(
            ([, params]) =>
                (params as { action: { data: { payload: Record<string, unknown> } } }).action
                    .data.payload,
        );
}

function lastEnvelopeFor(command: string): unknown {
    const calls = mockCallSpy.mock.calls.filter(
        ([method, params]) =>
            method === 'action.invoke' &&
            (params as { action?: { data?: { payload?: { command?: string } } } }).action?.data
                ?.payload?.command === command,
    );
    return calls.length > 0 ? calls[calls.length - 1][1] : undefined;
}

describe('BluetoothMenu App shell', () => {
    it('opens a discovery session on mount', async () => {
        render(App);
        await settle();
        expect(invokePayloads()).toContainEqual({ command: 'open_session' });
    });

    it('Escape closes the session before hiding by the canonical view name', async () => {
        render(App);
        await settle();
        mockCallSpy.mockClear();
        await fireEvent.keyDown(document, { key: 'Escape' });
        await tick();
        expect(invokePayloads()).toContainEqual({ command: 'close_session' });
        expect(mockCallSpy).toHaveBeenCalledWith('view.hide', {
            name: 'plugin/bluetooth-menu/bluetooth-menu',
        });
    });

    it('backdrop click dismisses; card clicks do not', async () => {
        const { container } = render(App);
        await settle();
        mockCallSpy.mockClear();
        await fireEvent.click(container.querySelector('.card') as HTMLElement);
        await tick();
        expect(mockCallSpy).not.toHaveBeenCalledWith('view.hide', expect.anything());
        await fireEvent.click(container.querySelector('.backdrop') as HTMLElement);
        await tick();
        expect(mockCallSpy).toHaveBeenCalledWith('view.hide', {
            name: 'plugin/bluetooth-menu/bluetooth-menu',
        });
    });

    it('the power switch sends the exact set_powered envelope', async () => {
        const { container } = render(App);
        await settle();
        const powerSwitch = container.querySelector('[data-action="power"]') as HTMLButtonElement;
        expect(powerSwitch).not.toBeNull();
        await fireEvent.click(powerSwitch);
        await tick();
        expect(lastEnvelopeFor('set_powered')).toEqual({
            provider: 'bluetooth',
            action: {
                kind: 'custom',
                data: {
                    kind: 'bluetooth',
                    payload: { command: 'set_powered', value: false },
                },
            },
        });
    });

    it('renders a service-unavailable card when bluetooth is unavailable', async () => {
        mockState = {
            available: false,
            powered: false,
            discovering: false,
            devices: [],
            adapter_path: '',
        };
        const { getByText } = render(App);
        await settle();
        expect(getByText(/bluetooth service unavailable/i)).not.toBeNull();
    });

    it('a state payload from the stream replaces the rendered state', async () => {
        const { container } = render(App);
        await settle();
        const updated = baseState();
        updated.powered = false;
        capturedSubscribeCallback!(updated);
        await settle();
        // The header toggle pill flips from "On" to "Off" once the streamed
        // state replaces the queried seed. Scoped to the pill because the
        // powered-off card also contains the word "off".
        expect((container.querySelector('.toggle-pill') as HTMLElement).textContent).toMatch(
            /off/i,
        );
    });
});
