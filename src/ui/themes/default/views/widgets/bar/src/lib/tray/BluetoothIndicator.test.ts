import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import BluetoothIndicator from './BluetoothIndicator.svelte';
import type { BluetoothState } from '../types';

function mockClient(): {
	client: any;
	emit: (p: BluetoothState) => Promise<void>;
} {
	let saved: ((p: unknown) => void) | undefined;
	const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
		saved = cb;
		return () => {};
	});
	const call = vi.fn().mockResolvedValue(undefined);
	const client = { call, subscribe, close: vi.fn() };
	const emit = async (p: BluetoothState) => {
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
			connected_devices: [],
		});
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
			connected_devices: [
				{ address: 'AA:BB:CC:DD:EE:FF', name: 'Headphones', battery_percent: null },
				{ address: 'BB:CC:DD:EE:FF:00', name: 'Mouse', battery_percent: 50 },
			],
		});
		const iconWrap = container.querySelector('.bluetooth-icon');
		expect(iconWrap).not.toBeNull();
		expect(iconWrap!.classList.contains('powered')).toBe(true);
		expect(iconWrap!.classList.contains('has-devices')).toBe(true);
		// Connected state shows through the accent color on the icon
		// itself; the device-count badge was removed since the names
		// are already in the tooltip.
	});

	it('tooltip lists connected device names', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BluetoothIndicator, { props: { client } });
		await emit({
			available: true,
			powered: true,
			discovering: false,
			connected_devices: [
				{ address: 'AA:BB:CC:DD:EE:FF', name: 'Headphones', battery_percent: null },
				{ address: 'BB:CC:DD:EE:FF:00', name: 'Mouse', battery_percent: 50 },
			],
		});
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
			connected_devices: [
				{ address: 'AA:BB:CC:DD:EE:FF', name: 'Headphones', battery_percent: null },
				{ address: 'BB:CC:DD:EE:FF:00', name: 'Mouse', battery_percent: 50 },
			],
		});
		const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
		expect(btn).not.toBeNull();
		expect(btn!.title).toContain('50%');
	});

	it('click invokes shell_command to launch blueman-manager', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BluetoothIndicator, { props: { client } });
		await emit({
			available: true,
			powered: true,
			discovering: false,
			connected_devices: [],
		});
		const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
		expect(btn).not.toBeNull();
		await fireEvent.click(btn!);
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'shell',
			action: {
				kind: 'shell',
				data: { command: ['blueman-manager'], terminal: false },
			},
		});
	});
});
