import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import type { WifiState } from './lib/types';

/*
 * Module-level mock state. Each test resets these via beforeEach so
 * the `vi.mock` factory below sees fresh values. The factory closes
 * over them by reference. Mirrors the power-menu App test pattern.
 */
let mockState: WifiState;
let mockCallSpy = vi.fn();
let mockSubscribeSpy = vi.fn();
let capturedSubscribeCallback: ((payload: unknown) => void) | null = null;
/* When set, the mock rejects connect commands with this value so the
 * error-classification path can be exercised. */
let connectRejection: unknown | null = null;

function radioOnState(): WifiState {
    return {
        available: true,
        radio_enabled: true,
        scanning: false,
        active: {
            ssid: 'Skynet_5G',
            signal_percent: 92,
            security: 'wpa3',
            details: null,
        },
        networks: [
            {
                ssid: 'Skynet_5G',
                bssid: 'a1',
                signal_percent: 92,
                security: 'wpa3',
                band: 'five',
                saved: true,
                active: true,
            },
            {
                ssid: 'CoffeeShopFree',
                bssid: 'c1',
                signal_percent: 55,
                security: 'open',
                band: 'two_four',
                saved: false,
                active: false,
            },
            {
                ssid: 'NETGEAR-Guest',
                bssid: 'd1',
                signal_percent: 41,
                security: 'wpa2',
                band: 'two_four',
                saved: false,
                active: false,
            },
        ],
        saved: [
            {
                id: 'Skynet_5G',
                ssid: 'Skynet_5G',
                security: 'wpa3',
                autoconnect: true,
                in_range: true,
            },
            {
                id: 'Office-Floor3',
                ssid: 'Office-Floor3',
                security: 'wpa2',
                autoconnect: false,
                in_range: false,
            },
        ],
    };
}

function radioOffState(): WifiState {
    return {
        available: true,
        radio_enabled: false,
        scanning: false,
        active: null,
        networks: [],
        saved: [],
    };
}

vi.mock('@quantum/client', () => ({
    createClient: () => ({
        call: (...args: unknown[]) => {
            mockCallSpy(...args);
            const [method, params] = args as [string, unknown];
            if (method === 'provider.query' && (params as { id?: string })?.id === 'wifi') {
                return Promise.resolve(mockState);
            }
            if (method === 'action.invoke' && connectRejection !== null) {
                const command = (
                    params as { action?: { data?: { payload?: { command?: string } } } }
                ).action?.data?.payload?.command;
                if (command === 'connect') {
                    return Promise.reject(connectRejection);
                }
            }
            return Promise.resolve(undefined);
        },
        subscribe: (...args: unknown[]) => {
            mockSubscribeSpy(...args);
            const [, callback] = args as [string, (payload: unknown) => void];
            capturedSubscribeCallback = callback;
            return () => {};
        },
        close: vi.fn(),
    }),
    __esModule: true,
}));

import App from './App.svelte';

beforeEach(() => {
    mockCallSpy = vi.fn();
    mockSubscribeSpy = vi.fn();
    capturedSubscribeCallback = null;
    connectRejection = null;
    mockState = radioOnState();
});

async function settle(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 10));
    await tick();
    await tick();
}

/** The action.invoke calls recorded by the spy, newest last. */
function invokeCalls(): Array<{
    provider?: string;
    action?: { kind?: string; data?: { kind?: string; payload?: Record<string, unknown> } };
}> {
    return mockCallSpy.mock.calls
        .filter(([method]) => method === 'action.invoke')
        .map(([, params]) => params as never);
}

/** Find the most recent action.invoke whose payload.command matches. */
function lastCommand(command: string):
    | { kind?: string; data?: { kind?: string; payload?: Record<string, unknown> } }
    | undefined {
    const matches = invokeCalls().filter(
        (params) => params.action?.data?.payload?.command === command,
    );
    return matches.length > 0 ? matches[matches.length - 1].action : undefined;
}

describe('WifiMenu App', () => {
    it('Escape stops the scan session and hides by the canonical view name', async () => {
        render(App);
        await settle();
        mockCallSpy.mockClear();
        await fireEvent.keyDown(document, { key: 'Escape' });
        await tick();
        // The overlay is kept warm (hidden, not destroyed), so it must stop
        // the background scan itself rather than rely on unmount cleanup.
        expect(lastCommand('close_session')).toBeDefined();
        // And it must hide by the name the bar opens it under, or the surface
        // never hides and the overlay cannot be closed.
        expect(mockCallSpy).toHaveBeenCalledWith('view.hide', {
            name: 'plugin/wifi-menu/wifi-menu',
        });
    });

    it('radio-off state renders a "Turn on WiFi" affordance that sends set_radio enabled:true', async () => {
        mockState = radioOffState();
        const { getByText } = render(App);
        await settle();

        const turnOn = getByText(/turn on wifi/i);
        expect(turnOn).not.toBeNull();

        await fireEvent.click(turnOn);
        await tick();
        await tick();

        const action = lastCommand('set_radio');
        expect(action).toBeDefined();
        expect(action).toEqual({
            kind: 'custom',
            data: { kind: 'wifi', payload: { command: 'set_radio', enabled: true } },
        });
    });

    it('clicking a secured unsaved network reveals the password form and does NOT send connect', async () => {
        const { container, queryByText } = render(App);
        await settle();

        // NETGEAR-Guest is wpa2 + unsaved -> should expand the form.
        const row = container.querySelector('[data-bssid="d1"]') as HTMLElement;
        expect(row).not.toBeNull();
        await fireEvent.click(row);
        await tick();
        await tick();

        // The inline password form is now present.
        expect(container.querySelector('.pwform')).not.toBeNull();
        expect(queryByText(/password for/i)).not.toBeNull();

        // No connect command was sent yet.
        expect(lastCommand('connect')).toBeUndefined();
    });

    it('clicking an open network sends connect with the ssid and no password', async () => {
        const { container } = render(App);
        await settle();

        const row = container.querySelector('[data-bssid="c1"]') as HTMLElement;
        expect(row).not.toBeNull();
        await fireEvent.click(row);
        await tick();
        await tick();

        const action = lastCommand('connect');
        expect(action).toBeDefined();
        const payload = action!.data!.payload as Record<string, unknown>;
        expect(payload.command).toBe('connect');
        expect(payload.ssid).toBe('CoffeeShopFree');
        expect('password' in payload).toBe(false);
    });

    it('submitting the password form sends connect with ssid and password', async () => {
        const { container } = render(App);
        await settle();

        const row = container.querySelector('[data-bssid="d1"]') as HTMLElement;
        await fireEvent.click(row);
        await tick();
        await tick();

        const input = container.querySelector('.pwform input') as HTMLInputElement;
        expect(input).not.toBeNull();
        await fireEvent.input(input, { target: { value: 'hunter2' } });
        await tick();

        const submit = container.querySelector('.pwform .btn') as HTMLButtonElement;
        await fireEvent.click(submit);
        await tick();
        await tick();

        const action = lastCommand('connect');
        expect(action).toBeDefined();
        const payload = action!.data!.payload as Record<string, unknown>;
        expect(payload.ssid).toBe('NETGEAR-Guest');
        expect(payload.password).toBe('hunter2');
    });

    it('emits the exact nested custom envelope shape for action.invoke', async () => {
        const { container } = render(App);
        await settle();

        const row = container.querySelector('[data-bssid="c1"]') as HTMLElement;
        await fireEvent.click(row);
        await tick();
        await tick();

        const call = mockCallSpy.mock.calls.find(
            ([method, params]) =>
                method === 'action.invoke' &&
                (params as { action?: { data?: { payload?: { command?: string } } } }).action?.data
                    ?.payload?.command === 'connect',
        );
        expect(call).toBeDefined();
        const [, params] = call!;
        expect(params).toEqual({
            provider: 'wifi',
            action: {
                kind: 'custom',
                data: {
                    kind: 'wifi',
                    payload: { command: 'connect', ssid: 'CoffeeShopFree' },
                },
            },
        });
    });

    it('forget from a saved row sends forget with the network id', async () => {
        const { container } = render(App);
        await settle();

        // Switch to the Saved tab.
        const savedTab = container.querySelector('[data-tab="saved"]') as HTMLButtonElement;
        await fireEvent.click(savedTab);
        await tick();
        await tick();

        const forgetButton = container.querySelector(
            '[data-saved-id="Office-Floor3"] [data-action="forget"]',
        ) as HTMLButtonElement;
        expect(forgetButton).not.toBeNull();
        await fireEvent.click(forgetButton);
        await tick();
        await tick();

        const action = lastCommand('forget');
        expect(action).toBeDefined();
        expect(action!.data!.payload).toEqual({ command: 'forget', id: 'Office-Floor3' });
    });

    it('shows "Incorrect password" when the connect call rejects with that reason', async () => {
        // Daemon-style rejection: the JSON-RPC error carries the reason.
        connectRejection = { code: -32000, message: 'connect failed: incorrect_password' };

        const { container, queryByText } = render(App);
        await settle();

        // Expand the password form for a secured, unsaved network.
        const row = container.querySelector('[data-bssid="d1"]') as HTMLElement;
        await fireEvent.click(row);
        await tick();
        await tick();

        const input = container.querySelector('.pwform input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'wrongpass' } });
        await tick();

        const submit = container.querySelector('.pwform .btn') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        // The form stays open and surfaces the incorrect-password error
        // in its own error element (the row also flags it inline).
        const form = container.querySelector('.pwform');
        expect(form).not.toBeNull();
        const formError = form!.querySelector('.pwerror') as HTMLElement;
        expect(formError).not.toBeNull();
        expect(formError.textContent?.toLowerCase()).toContain('incorrect password');
    });

    it('re-renders the network list when a new WifiState arrives on the stream', async () => {
        const { container } = render(App);
        await settle();

        // The pushed network is not present in the initial fixture.
        expect(container.querySelector('[data-bssid="z9"]')).toBeNull();
        expect(capturedSubscribeCallback).not.toBeNull();

        const updated = radioOnState();
        updated.active = null;
        updated.networks = [
            {
                ssid: 'FreshlyAppeared',
                bssid: 'z9',
                signal_percent: 70,
                security: 'wpa2',
                band: 'five',
                saved: false,
                active: false,
            },
        ];
        capturedSubscribeCallback!(updated);
        await settle();

        const freshRow = container.querySelector('[data-bssid="z9"]');
        expect(freshRow).not.toBeNull();
        expect(freshRow?.textContent).toContain('FreshlyAppeared');
        // The old fixture rows are gone.
        expect(container.querySelector('[data-bssid="c1"]')).toBeNull();
    });
});
