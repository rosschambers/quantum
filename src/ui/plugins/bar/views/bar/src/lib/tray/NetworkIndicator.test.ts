import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import { closeContextMenu } from '@quantum/client';
import NetworkIndicator from './NetworkIndicator.svelte';
import type { NetworkState } from '../types';

function menuItem(text: string): HTMLButtonElement | undefined {
    return Array.from(
        document.querySelectorAll('[data-quantum-context-menu] [role="menuitem"]'),
    ).find((el) => el.textContent?.includes(text)) as HTMLButtonElement | undefined;
}

afterEach(() => {
    closeContextMenu();
    (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = undefined;
});

function mockClient(): {
    client: any;
    emit: (p: NetworkState) => Promise<void>;
} {
    let saved: ((p: unknown) => void) | undefined;
    const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
        saved = cb;
        return () => {};
    });
    const call = vi.fn().mockResolvedValue(undefined);
    const client = { call, subscribe, close: vi.fn() };
    const emit = async (p: NetworkState) => {
        await tick();
        saved?.(p);
        await tick();
    };
    return { client, emit };
}

describe('NetworkIndicator', () => {
    it('renders nothing when unavailable', () => {
        const { client } = mockClient();
        const { container } = render(NetworkIndicator, { props: { client } });
        expect(container.querySelector('.bar-button')).toBeNull();
    });

    it('renders the BarButton with ethernet icon when connected via ethernet', async () => {
        const { client, emit } = mockClient();
        const { container } = render(NetworkIndicator, { props: { client } });
        await emit({
            available: true,
            connectivity: 'full',
            primary: {
                kind: 'ethernet',
                id: 'eth0',
                ssid: null,
            },
            wifi_enabled: false,
            wifi_signal_percent: null,
        });
        const btn = container.querySelector('.bar-button');
        expect(btn).not.toBeNull();
        const iconWrap = btn!.querySelector('.network-icon');
        expect(iconWrap).not.toBeNull();
        expect(iconWrap!.classList.contains('connected')).toBe(true);
        expect(iconWrap!.classList.contains('full')).toBe(true);
        // SVG icon always present.
        expect(btn!.querySelector('svg.icon')).not.toBeNull();
    });

    it('marks the icon connected+full when on wifi with full connectivity', async () => {
        const { client, emit } = mockClient();
        const { container } = render(NetworkIndicator, { props: { client } });
        await emit({
            available: true,
            connectivity: 'full',
            primary: {
                kind: 'wifi',
                id: 'wlan0',
                ssid: 'HomeWifi',
            },
            wifi_enabled: true,
            wifi_signal_percent: 90,
        });
        const iconWrap = container.querySelector('.network-icon');
        expect(iconWrap).not.toBeNull();
        expect(iconWrap!.classList.contains('connected')).toBe(true);
        expect(iconWrap!.classList.contains('full')).toBe(true);
    });

    it('does not mark the icon full when wifi connectivity is limited', async () => {
        const { client, emit } = mockClient();
        const { container } = render(NetworkIndicator, { props: { client } });
        await emit({
            available: true,
            connectivity: 'limited',
            primary: {
                kind: 'wifi',
                id: 'wlan0',
                ssid: 'FarWifi',
            },
            wifi_enabled: true,
            wifi_signal_percent: 30,
        });
        const iconWrap = container.querySelector('.network-icon');
        expect(iconWrap).not.toBeNull();
        expect(iconWrap!.classList.contains('connected')).toBe(true);
        expect(iconWrap!.classList.contains('full')).toBe(false);
    });

    it('tooltip includes connection name and connectivity', async () => {
        const { client, emit } = mockClient();
        const { container } = render(NetworkIndicator, { props: { client } });
        await emit({
            available: true,
            connectivity: 'full',
            primary: {
                kind: 'wifi',
                id: 'wlan0',
                ssid: 'HomeWifi',
            },
            wifi_enabled: true,
            wifi_signal_percent: 90,
        });
        const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
        expect(btn).not.toBeNull();
        expect(btn!.title).toContain('HomeWifi');
        expect(btn!.title).toContain('full');
    });

    it('click opens the wifi-menu overlay (bare name when monitor unknown)', async () => {
        (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = undefined;
        const { client, emit } = mockClient();
        const { container } = render(NetworkIndicator, { props: { client } });
        await emit({
            available: true,
            connectivity: 'full',
            primary: {
                kind: 'ethernet',
                id: 'eth0',
                ssid: null,
            },
            wifi_enabled: false,
            wifi_signal_percent: null,
        });
        const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
        expect(btn).not.toBeNull();
        await fireEvent.click(btn!);
        await tick();
        expect(client.call).toHaveBeenCalledWith('view.show', { name: 'widgets/wifi-menu' });
    });

    it('click appends the @monitor suffix when __quantum_monitor is set', async () => {
        (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = 'DP-1';
        const { client, emit } = mockClient();
        const { container } = render(NetworkIndicator, { props: { client } });
        await emit({
            available: true,
            connectivity: 'full',
            primary: { kind: 'ethernet', id: 'eth0', ssid: null },
            wifi_enabled: false,
            wifi_signal_percent: null,
        });
        const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
        expect(btn).not.toBeNull();
        await fireEvent.click(btn!);
        await tick();
        expect(client.call).toHaveBeenCalledWith('view.show', { name: 'widgets/wifi-menu@DP-1' });
        (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = undefined;
    });

    it('toggles Wi-Fi from the right-click menu with the negated value', async () => {
        const { client, emit } = mockClient();
        const { container } = render(NetworkIndicator, { props: { client } });
        await emit({
            available: true,
            connectivity: 'full',
            primary: { kind: 'wifi', id: 'wlan0', ssid: 'HomeWifi' },
            wifi_enabled: true,
            wifi_signal_percent: 90,
        });

        const btn = container.querySelector('.bar-button') as HTMLButtonElement;
        await fireEvent.contextMenu(btn);
        await tick();

        const toggle = menuItem('Turn Wi-Fi off');
        expect(toggle).toBeTruthy();
        await fireEvent.click(toggle as HTMLButtonElement);
        await tick();

        expect(client.call).toHaveBeenCalledWith('action.invoke', {
            provider: 'network',
            action: {
                kind: 'custom',
                data: {
                    kind: 'network',
                    payload: { command: 'set_wifi_enabled', value: false },
                },
            },
        });
    });
});
