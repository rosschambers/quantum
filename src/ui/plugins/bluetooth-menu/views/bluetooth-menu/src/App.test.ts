import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import type { BluetoothState } from './lib/types';

let mockState: BluetoothState;
let mockCallSpy = vi.fn();
let capturedSubscribeCallback: ((payload: unknown) => void) | null = null;
let connectRejection: unknown | null = null;
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
            {
                address: 'AA:00:00:00:00:05',
                name: '',
                battery_percent: null,
                paired: false,
                trusted: false,
                connected: false,
                icon: null,
                rssi: -55,
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
            if (method === 'action.invoke' && connectRejection !== null) {
                const command = (
                    params as { action?: { data?: { payload?: { command?: string } } } }
                ).action?.data?.payload?.command;
                if (command === 'pair' || command === 'connect') {
                    return Promise.reject(connectRejection);
                }
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
    connectRejection = null;
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

    it('renders no window-level close button (Escape and backdrop only)', async () => {
        const { container } = render(App);
        await settle();
        expect(container.querySelector('[data-action="close"]')).toBeNull();
        expect(container.querySelector('.close-btn')).toBeNull();
    });
});

describe('BluetoothMenu devices', () => {
    it('renders devices into the right sections', async () => {
        const { container } = render(App);
        await settle();
        const inSection = (section: string, address: string) =>
            container.querySelector(`[data-section="${section}"] [data-address="${address}"]`);
        expect(inSection('connected', 'AA:00:00:00:00:01')).not.toBeNull();
        expect(inSection('known', 'AA:00:00:00:00:02')).not.toBeNull();
        expect(inSection('available', 'AA:00:00:00:00:03')).not.toBeNull();
        expect(inSection('available', 'AA:00:00:00:00:04')).not.toBeNull();
    });

    it('sorts available devices by descending RSSI', async () => {
        const { container } = render(App);
        await settle();
        const addresses = Array.from(
            container.querySelectorAll('[data-section="available"] [data-address]'),
        ).map((row) => row.getAttribute('data-address'));
        // Earbuds (-40) is stronger than Television (-70).
        expect(addresses).toEqual(['AA:00:00:00:00:04', 'AA:00:00:00:00:03']);
    });

    it('clicking an available row pairs, then connects, then trusts', async () => {
        const { container } = render(App);
        await settle();
        mockCallSpy.mockClear();
        const row = container.querySelector(
            '[data-section="available"] [data-address="AA:00:00:00:00:04"]',
        ) as HTMLElement;
        await fireEvent.click(row);
        await settle();
        const commands = invokePayloads();
        expect(commands).toEqual([
            { command: 'pair', address: 'AA:00:00:00:00:04' },
            { command: 'connect', address: 'AA:00:00:00:00:04' },
            { command: 'set_trusted', address: 'AA:00:00:00:00:04', value: true },
        ]);
    });

    it('a rejected pair shows an inline error on the row', async () => {
        connectRejection = { code: -32000, message: 'pairing failed' };
        const { container } = render(App);
        await settle();
        const row = container.querySelector(
            '[data-section="available"] [data-address="AA:00:00:00:00:04"]',
        ) as HTMLElement;
        await fireEvent.click(row);
        await settle();
        const status = container.querySelector(
            '[data-address="AA:00:00:00:00:04"] .row-status',
        ) as HTMLElement;
        expect(status?.textContent?.toLowerCase()).toContain('failed');
    });

    it('disconnect on a connected row sends disconnect', async () => {
        const { container } = render(App);
        await settle();
        const button = container.querySelector(
            '[data-address="AA:00:00:00:00:01"] [data-action="disconnect"]',
        ) as HTMLButtonElement;
        await fireEvent.click(button);
        await tick();
        expect(invokePayloads()).toContainEqual({
            command: 'disconnect',
            address: 'AA:00:00:00:00:01',
        });
    });

    it('connect on a paired row sends connect', async () => {
        const { container } = render(App);
        await settle();
        const button = container.querySelector(
            '[data-address="AA:00:00:00:00:02"] [data-action="connect"]',
        ) as HTMLButtonElement;
        await fireEvent.click(button);
        await settle();
        expect(invokePayloads()).toContainEqual({
            command: 'connect',
            address: 'AA:00:00:00:00:02',
        });
    });

    it('connected rows expose inline disconnect, trust, and remove buttons with tooltips', async () => {
        const { container } = render(App);
        await settle();
        const row = container.querySelector('[data-address="AA:00:00:00:00:01"]') as HTMLElement;
        const disconnect = row.querySelector('[data-action="disconnect"]') as HTMLButtonElement;
        const trust = row.querySelector('[data-action="trust"]') as HTMLButtonElement;
        const remove = row.querySelector('[data-action="remove"]') as HTMLButtonElement;
        expect(disconnect).not.toBeNull();
        expect(trust).not.toBeNull();
        expect(remove).not.toBeNull();
        expect(disconnect.getAttribute('title')).toBeTruthy();
        expect(remove.getAttribute('title')).toBeTruthy();
    });

    it('inline remove on a known row sends remove', async () => {
        const { container } = render(App);
        await settle();
        const remove = container.querySelector(
            '[data-address="AA:00:00:00:00:02"] [data-action="remove"]',
        ) as HTMLButtonElement;
        await fireEvent.click(remove);
        await tick();
        expect(invokePayloads()).toContainEqual({
            command: 'remove',
            address: 'AA:00:00:00:00:02',
        });
    });

    it('right-click offers trust toggle and remove through the context menu', async () => {
        const { container } = render(App);
        await settle();
        const row = container.querySelector(
            '[data-address="AA:00:00:00:00:02"]',
        ) as HTMLElement;
        await fireEvent.contextMenu(row);
        await tick();
        expect(openContextMenuSpy).toHaveBeenCalled();
        const items = openContextMenuSpy.mock.calls[0][1] as Array<{
            label?: string;
            onSelect?: () => void;
        }>;
        const trust = items.find((item) => item.label === 'Trust device');
        const remove = items.find((item) => item.label === 'Remove device');
        expect(trust).toBeDefined();
        expect(remove).toBeDefined();
        trust!.onSelect!();
        remove!.onSelect!();
        await tick();
        expect(invokePayloads()).toContainEqual({
            command: 'set_trusted',
            address: 'AA:00:00:00:00:02',
            value: true,
        });
        expect(invokePayloads()).toContainEqual({
            command: 'remove',
            address: 'AA:00:00:00:00:02',
        });
    });
});

describe('BluetoothMenu unknown devices expander', () => {
    it('hides unnamed devices behind a collapsed expander with a count', async () => {
        const { container } = render(App);
        await settle();
        const available = container.querySelector('[data-section="available"]') as HTMLElement;
        // Named devices are visible; the unnamed MAC-only one is not.
        expect(available.querySelector('[data-address="AA:00:00:00:00:04"]')).not.toBeNull();
        expect(available.querySelector('[data-address="AA:00:00:00:00:05"]')).toBeNull();
        const toggle = available.querySelector('[data-action="toggle-unknown"]') as HTMLButtonElement;
        expect(toggle).not.toBeNull();
        expect(toggle.textContent).toContain('1');
    });

    it('reveals unnamed devices when the expander is opened', async () => {
        const { container } = render(App);
        await settle();
        const toggle = container.querySelector('[data-action="toggle-unknown"]') as HTMLButtonElement;
        await fireEvent.click(toggle);
        await tick();
        expect(
            container.querySelector('[data-section="available"] [data-address="AA:00:00:00:00:05"]'),
        ).not.toBeNull();
    });
});

describe('BluetoothMenu pairing dialog', () => {
    function pushPairingRequest(request: string, passkey: number | null = 123456): void {
        capturedSubscribeCallback!({
            event: 'pairing_request',
            request,
            address: 'AA:00:00:00:00:04',
            device_path: '/org/bluez/hci0/dev_AA_00_00_00_00_04',
            passkey,
            service_uuid: null,
        });
    }

    it('confirm request shows the passkey and Confirm sends an accepting pairing_response', async () => {
        const { container, getByText } = render(App);
        await settle();
        pushPairingRequest('confirm');
        await settle();
        expect(container.querySelector('.pairing-dialog')).not.toBeNull();
        expect(getByText('123456')).not.toBeNull();
        await fireEvent.click(getByText('Confirm'));
        await tick();
        expect(invokePayloads()).toContainEqual({
            command: 'pairing_response',
            address: 'AA:00:00:00:00:04',
            accept: true,
        });
        expect(container.querySelector('.pairing-dialog')).toBeNull();
    });

    it('confirm request Cancel sends a rejecting pairing_response', async () => {
        const { container, getByText } = render(App);
        await settle();
        pushPairingRequest('confirm');
        await settle();
        await fireEvent.click(getByText('Cancel'));
        await tick();
        expect(invokePayloads()).toContainEqual({
            command: 'pairing_response',
            address: 'AA:00:00:00:00:04',
            accept: false,
        });
        expect(container.querySelector('.pairing-dialog')).toBeNull();
    });

    it('request_pin submits the typed pin', async () => {
        const { container, getByText } = render(App);
        await settle();
        pushPairingRequest('request_pin', null);
        await settle();
        const input = container.querySelector('.pairing-dialog input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: '0000' } });
        await fireEvent.click(getByText('Pair'));
        await tick();
        expect(invokePayloads()).toContainEqual({
            command: 'pairing_response',
            address: 'AA:00:00:00:00:04',
            accept: true,
            pin: '0000',
        });
    });

    it('request_passkey submits the typed passkey as a number', async () => {
        const { container, getByText } = render(App);
        await settle();
        pushPairingRequest('request_passkey', null);
        await settle();
        const input = container.querySelector('.pairing-dialog input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: '654321' } });
        await fireEvent.click(getByText('Pair'));
        await tick();
        expect(invokePayloads()).toContainEqual({
            command: 'pairing_response',
            address: 'AA:00:00:00:00:04',
            accept: true,
            passkey: 654321,
        });
    });

    it('pairing_cancelled dismisses the dialog without a response', async () => {
        const { container } = render(App);
        await settle();
        pushPairingRequest('confirm');
        await settle();
        expect(container.querySelector('.pairing-dialog')).not.toBeNull();
        capturedSubscribeCallback!({ event: 'pairing_cancelled', address: 'AA:00:00:00:00:04' });
        await settle();
        expect(container.querySelector('.pairing-dialog')).toBeNull();
        expect(
            invokePayloads().filter((payload) => payload.command === 'pairing_response'),
        ).toEqual([]);
    });

    it('authorize_service offers Allow and Deny', async () => {
        const { getByText } = render(App);
        await settle();
        pushPairingRequest('authorize_service', null);
        await settle();
        await fireEvent.click(getByText('Allow'));
        await tick();
        expect(invokePayloads()).toContainEqual({
            command: 'pairing_response',
            address: 'AA:00:00:00:00:04',
            accept: true,
        });
    });
});
