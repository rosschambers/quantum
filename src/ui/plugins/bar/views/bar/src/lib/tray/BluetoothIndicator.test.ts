import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import { closeContextMenu } from '@quantum/client';
import BluetoothIndicator from './BluetoothIndicator.svelte';
import type { BluetoothDevice, BluetoothState } from '../types';

function device(overrides: Partial<BluetoothDevice> = {}): BluetoothDevice {
    return {
        address: 'AA:BB:CC:DD:EE:FF',
        name: 'Headphones',
        battery_percent: null,
        paired: true,
        trusted: true,
        connected: true,
        icon: 'audio-headset',
        rssi: null,
        ...overrides,
    };
}

function menuItem(text: string): HTMLButtonElement | undefined {
    return Array.from(
        document.querySelectorAll('[data-quantum-context-menu] [role="menuitem"]'),
    ).find((el) => el.textContent?.includes(text)) as HTMLButtonElement | undefined;
}

afterEach(() => {
    closeContextMenu();
});

function mockClient(): {
    client: any;
    emit: (p: unknown) => Promise<void>;
} {
    let saved: ((p: unknown) => void) | undefined;
    const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
        saved = cb;
        return () => {};
    });
    const call = vi.fn().mockResolvedValue(undefined);
    const client = { call, subscribe, close: vi.fn() };
    const emit = async (p: unknown) => {
        await tick();
        saved?.(p);
        await tick();
    };
    return { client, emit };
}

describe('BluetoothIndicator', () => {
    it('renders nothing when unavailable', () => {
        const { client } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        expect(container.querySelector('.bar-button')).toBeNull();
    });

    it('renders the BarButton with BT icon when available and unpowered', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: false,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [],
        } satisfies BluetoothState);
        const btn = container.querySelector('.bar-button');
        expect(btn).not.toBeNull();
        const iconWrap = btn!.querySelector('.bluetooth-icon');
        expect(iconWrap).not.toBeNull();
        expect(iconWrap!.classList.contains('powered')).toBe(false);
        // SVG bluetooth icon (always present, color shifts when powered).
        expect(btn!.querySelector('svg.icon')).not.toBeNull();
    });

    it('renders the BT icon highlighted when devices are connected', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: true,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [
                device({ address: 'AA:BB:CC:DD:EE:FF', name: 'Headphones' }),
                device({
                    address: 'BB:CC:DD:EE:FF:00',
                    name: 'Mouse',
                    battery_percent: 50,
                    icon: 'input-mouse',
                }),
            ],
        } satisfies BluetoothState);
        const iconWrap = container.querySelector('.bluetooth-icon');
        expect(iconWrap).not.toBeNull();
        expect(iconWrap!.classList.contains('powered')).toBe(true);
        expect(iconWrap!.classList.contains('has-devices')).toBe(true);
    });

    it('tooltip lists connected device names', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: true,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [
                device({ address: 'AA:BB:CC:DD:EE:FF', name: 'Headphones' }),
                device({
                    address: 'BB:CC:DD:EE:FF:00',
                    name: 'Mouse',
                    battery_percent: 50,
                    icon: 'input-mouse',
                }),
            ],
        } satisfies BluetoothState);
        const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
        expect(btn).not.toBeNull();
        expect(btn!.title).toContain('Headphones');
        expect(btn!.title).toContain('Mouse');
    });

    it('tooltip shows battery percent when present', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: true,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [
                device({ address: 'AA:BB:CC:DD:EE:FF', name: 'Headphones' }),
                device({
                    address: 'BB:CC:DD:EE:FF:00',
                    name: 'Mouse',
                    battery_percent: 50,
                    icon: 'input-mouse',
                }),
            ],
        } satisfies BluetoothState);
        const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
        expect(btn).not.toBeNull();
        expect(btn!.title).toContain('50%');
    });

    it('excludes disconnected devices from the tooltip', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: true,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [
                device({ address: 'AA:BB:CC:DD:EE:FF', name: 'Headphones', connected: true }),
                device({
                    address: 'BB:CC:DD:EE:FF:00',
                    name: 'Idle Speaker',
                    connected: false,
                }),
            ],
        } satisfies BluetoothState);
        const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
        expect(btn).not.toBeNull();
        expect(btn!.title).toContain('Headphones');
        expect(btn!.title).not.toContain('Idle Speaker');
    });

    it('click opens the bluetooth manager window and kicks a discovery session', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: true,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [],
        } satisfies BluetoothState);
        const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
        expect(btn).not.toBeNull();
        await fireEvent.click(btn!);
        await tick();
        // No __quantum_monitor in jsdom, so monitorView returns the base name.
        expect(client.call).toHaveBeenCalledWith('view.show', {
            name: 'plugin/bluetooth-menu/bluetooth-menu',
        });
        expect(client.call).toHaveBeenCalledWith('action.invoke', {
            provider: 'bluetooth',
            action: {
                kind: 'custom',
                data: { kind: 'bluetooth', payload: { command: 'open_session' } },
            },
        });
    });

    it('a pairing event on the channel does not clobber indicator state', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: true,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [],
        } satisfies BluetoothState);
        await emit({
            event: 'pairing_request',
            request: 'confirm',
            address: 'AA:BB:CC:DD:EE:FF',
            device_path: '/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF',
            passkey: 123456,
            service_uuid: null,
        });
        // The indicator is still available (its state was not replaced by the
        // pairing event, which carries no `available` field).
        expect(container.querySelector('.bar-button')).not.toBeNull();
    });

    it('toggles the radio from the right-click menu with the negated value', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: false,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [],
        } satisfies BluetoothState);

        const btn = container.querySelector('.bar-button') as HTMLButtonElement;
        await fireEvent.contextMenu(btn);
        await tick();

        const toggle = menuItem('Turn Bluetooth on');
        expect(toggle).toBeTruthy();
        await fireEvent.click(toggle as HTMLButtonElement);
        await tick();

        expect(client.call).toHaveBeenCalledWith('action.invoke', {
            provider: 'bluetooth',
            action: {
                kind: 'custom',
                data: { kind: 'bluetooth', payload: { command: 'set_powered', value: true } },
            },
        });
    });

    it('the right-click Open Bluetooth manager item opens the window', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: true,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [],
        } satisfies BluetoothState);

        const btn = container.querySelector('.bar-button') as HTMLButtonElement;
        await fireEvent.contextMenu(btn);
        await tick();

        const open = menuItem('Open Bluetooth manager');
        expect(open).toBeTruthy();
        await fireEvent.click(open as HTMLButtonElement);
        await tick();

        expect(client.call).toHaveBeenCalledWith('view.show', {
            name: 'plugin/bluetooth-menu/bluetooth-menu',
        });
    });

    it('lists a disconnect item per connected device', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BluetoothIndicator, { props: { client } });
        await emit({
            available: true,
            powered: true,
            discovering: false,
            adapter_path: '/org/bluez/hci0',
            devices: [device({ address: 'AA:BB:CC:DD:EE:FF', name: 'Headphones' })],
        } satisfies BluetoothState);

        const btn = container.querySelector('.bar-button') as HTMLButtonElement;
        await fireEvent.contextMenu(btn);
        await tick();

        const disconnect = menuItem('Disconnect Headphones');
        expect(disconnect).toBeTruthy();
        await fireEvent.click(disconnect as HTMLButtonElement);
        await tick();

        expect(client.call).toHaveBeenCalledWith('action.invoke', {
            provider: 'bluetooth',
            action: {
                kind: 'custom',
                data: {
                    kind: 'bluetooth',
                    payload: { command: 'disconnect', address: 'AA:BB:CC:DD:EE:FF' },
                },
            },
        });
    });
});
