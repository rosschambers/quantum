import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import PowerProfileIndicator from './PowerProfileIndicator.svelte';
import type { PowerProfileState } from '../types';

function mockClient(): {
	client: any;
	emit: (p: PowerProfileState) => Promise<void>;
} {
	let saved: ((p: unknown) => void) | undefined;
	const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
		saved = cb;
		return () => {};
	});
	const call = vi.fn().mockResolvedValue(undefined);
	const client = { call, subscribe, close: vi.fn() };
	const emit = async (p: PowerProfileState) => {
		await tick();
		saved?.(p);
		await tick();
	};
	return { client, emit };
}

describe('PowerProfileIndicator', () => {
	it('renders nothing when unavailable', () => {
		const { client } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		expect(container.querySelector('.tray-icon')).toBeNull();
	});

	it('renders nothing when active is null even if available', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		await emit({
			available: true,
			active: null,
			profiles: ['balanced'],
			performance_inhibited: null,
		});
		expect(container.querySelector('.tray-icon')).toBeNull();
	});

	it('renders the arrow (balanced) glyph when active is balanced', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		await emit({
			available: true,
			active: 'balanced',
			profiles: ['power_saver', 'balanced', 'performance'],
			performance_inhibited: null,
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		// Nerd Font fa-arrow_right.
		expect(el!.querySelector('svg.ring .ring-label')!.textContent).toContain('\u{f061}');
	});

	it('renders the bolt (performance) glyph when active is performance', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		await emit({
			available: true,
			active: 'performance',
			profiles: ['power_saver', 'balanced', 'performance'],
			performance_inhibited: null,
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		// Nerd Font fa-bolt.
		expect(el!.querySelector('svg.ring .ring-label')!.textContent).toContain('\u{f0e7}');
	});

	it('renders the leaf (power_saver) glyph when active is power_saver', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		await emit({
			available: true,
			active: 'power_saver',
			profiles: ['power_saver', 'balanced', 'performance'],
			performance_inhibited: null,
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		// Nerd Font fa-leaf.
		expect(el!.querySelector('svg.ring .ring-label')!.textContent).toContain('\u{f06c}');
	});

	it('click cycles balanced -> performance', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		await emit({
			available: true,
			active: 'balanced',
			profiles: ['power_saver', 'balanced', 'performance'],
			performance_inhibited: null,
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.click(el!);
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'power_profile',
			action: {
				kind: 'custom',
				data: {
					kind: 'power_profile',
					payload: { command: 'set', profile: 'performance' },
				},
			},
		});
	});

	it('click cycles performance -> power_saver', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		await emit({
			available: true,
			active: 'performance',
			profiles: ['power_saver', 'balanced', 'performance'],
			performance_inhibited: null,
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.click(el!);
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'power_profile',
			action: {
				kind: 'custom',
				data: {
					kind: 'power_profile',
					payload: { command: 'set', profile: 'power_saver' },
				},
			},
		});
	});

	it('click skips performance when performance_inhibited is set', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		await emit({
			available: true,
			active: 'balanced',
			profiles: ['power_saver', 'balanced', 'performance'],
			performance_inhibited: 'thermald',
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.click(el!);
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'power_profile',
			action: {
				kind: 'custom',
				data: {
					kind: 'power_profile',
					payload: { command: 'set', profile: 'power_saver' },
				},
			},
		});
	});

	it('click skips profiles missing from profiles array', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		await emit({
			available: true,
			active: 'balanced',
			profiles: ['balanced', 'performance'],
			performance_inhibited: null,
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.click(el!);
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'power_profile',
			action: {
				kind: 'custom',
				data: {
					kind: 'power_profile',
					payload: { command: 'set', profile: 'performance' },
				},
			},
		});
		// Reset and test next cycle
		client.call.mockClear();
		await emit({
			available: true,
			active: 'performance',
			profiles: ['balanced', 'performance'],
			performance_inhibited: null,
		});
		fireEvent.click(el!);
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'power_profile',
			action: {
				kind: 'custom',
				data: {
					kind: 'power_profile',
					payload: { command: 'set', profile: 'balanced' },
				},
			},
		});
	});

	it('tooltip mentions inhibitor when present', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerProfileIndicator, { props: { client } });
		await emit({
			available: true,
			active: 'performance',
			profiles: ['power_saver', 'balanced', 'performance'],
			performance_inhibited: 'thermald',
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.getAttribute('title')).toContain('thermald');
	});
});
