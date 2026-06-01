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
		expect(container.querySelector('.tray-icon')).toBeNull();
	});

	it('renders the BT icon dimmed when powered is false', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BluetoothIndicator, { props: { client } });
		await emit({
			available: true,
			powered: false,
			discovering: false,
			connected_devices: [],
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.classList.contains('powered')).toBe(false);
		// Nerd Font fa-bluetooth glyph (PUA codepoint).
		expect(el!.querySelector('svg.ring .ring-label')!.textContent).toContain('\u{f293}');
		expect(el!.querySelector('.badge')).toBeNull();
	});

	it('renders the BT icon highlighted with a device count badge when devices connected', async () => {
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.classList.contains('powered')).toBe(true);
		expect(el!.classList.contains('has-devices')).toBe(true);
		const badge = el!.querySelector('.badge');
		expect(badge).not.toBeNull();
		expect(badge!.textContent).toBe('2');
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.title).toContain('Headphones');
		expect(el!.title).toContain('Mouse');
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.title).toContain('50%');
	});

	it('left click toggles powered', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BluetoothIndicator, { props: { client } });
		await emit({
			available: true,
			powered: true,
			discovering: false,
			connected_devices: [],
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.click(el!);
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
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

	it('right click does not invoke action', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BluetoothIndicator, { props: { client } });
		await emit({
			available: true,
			powered: true,
			discovering: false,
			connected_devices: [],
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.contextMenu(el!);
		await tick();
		expect(client.call).not.toHaveBeenCalledWith('action.invoke', expect.anything());
	});
});
