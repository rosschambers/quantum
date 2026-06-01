import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import NetworkIndicator from './NetworkIndicator.svelte';
import type { NetworkState } from '../types';

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
		expect(container.querySelector('.tray-icon')).toBeNull();
	});

	it('renders ethernet icon with a fully-filled ring when connected via ethernet', async () => {
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		// Ethernet uses the Nerd Font fa-plug glyph and renders a full ring.
		expect(el!.querySelector('svg.ring .ring-label')!.textContent).toContain('\u{f0e8}');
		const fill = el!.querySelector('svg.ring .ring-fill');
		const off = Number(fill!.getAttribute('stroke-dashoffset'));
		expect(off).toBeCloseTo(0, 1);
	});

	it('drives the ring fill by signal strength on wifi', async () => {
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
		const fill = container.querySelector('.tray-icon svg.ring .ring-fill');
		const circ = Number(fill!.getAttribute('stroke-dasharray'));
		const off = Number(fill!.getAttribute('stroke-dashoffset'));
		// 90% -> dashoffset is 10% of the circumference.
		expect(off).toBeCloseTo(circ * 0.1, 1);
	});

	it('drives the ring fill by signal strength when wifi is weak', async () => {
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
		const fill = container.querySelector('.tray-icon svg.ring .ring-fill');
		const circ = Number(fill!.getAttribute('stroke-dasharray'));
		const off = Number(fill!.getAttribute('stroke-dashoffset'));
		// 30% -> dashoffset is 70% of the circumference.
		expect(off).toBeCloseTo(circ * 0.7, 1);
	});

	it('left click invokes set_wifi_enabled with toggled value', async () => {
		const { client, emit } = mockClient();
		const { container } = render(NetworkIndicator, { props: { client } });
		await emit({
			available: true,
			connectivity: 'full',
			primary: {
				kind: 'wifi',
				id: 'wlan0',
				ssid: 'Home',
			},
			wifi_enabled: true,
			wifi_signal_percent: 80,
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.click(el!);
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

	it('right click does not call action.invoke', async () => {
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.contextMenu(el!);
		await tick();
		expect(client.call).not.toHaveBeenCalledWith('action.invoke', expect.anything());
	});
});
