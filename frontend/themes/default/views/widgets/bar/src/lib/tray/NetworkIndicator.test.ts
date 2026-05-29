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

	it('renders ethernet icon when connected via ethernet', async () => {
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
		expect(el!.textContent).toContain('⊞');
	});

	it('renders 3-bar wifi when signal >= 75', async () => {
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.textContent).toContain('▮▮▮');
	});

	it('renders 1-bar wifi when signal 25-49', async () => {
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.textContent).toContain('▮▯▯');
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
