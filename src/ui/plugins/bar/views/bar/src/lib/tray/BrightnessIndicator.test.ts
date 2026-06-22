import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import { closeContextMenu } from '@quantum/client';
import BrightnessIndicator from './BrightnessIndicator.svelte';
import type { BrightnessState } from '../types';

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
	emit: (p: BrightnessState) => Promise<void>;
} {
	let saved: ((p: unknown) => void) | undefined;
	const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
		saved = cb;
		return () => {};
	});
	const call = vi.fn().mockResolvedValue(undefined);
	const client = { call, subscribe, close: vi.fn() };
	const emit = async (p: BrightnessState) => {
		await tick();
		saved?.(p);
		await tick();
	};
	return { client, emit };
}

describe('BrightnessIndicator', () => {
	it('renders nothing when unavailable', () => {
		const { client } = mockClient();
		const { container } = render(BrightnessIndicator, { props: { client } });
		expect(container.querySelector('.bar-button')).toBeNull();
	});

	it('renders nothing when displays array is empty even if available', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BrightnessIndicator, { props: { client } });
		await emit({
			available: true,
			displays: [],
		});
		expect(container.querySelector('.bar-button')).toBeNull();
	});

	it('renders an icon + ring at 50% brightness', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BrightnessIndicator, { props: { client } });
		await emit({
			available: true,
			displays: [
				{
					subsystem: 'backlight',
					name: 'intel_backlight',
					current: 5000,
					max: 10000,
				},
			],
		});
		const el = container.querySelector('.bar-button');
		expect(el).not.toBeNull();
		expect(el!.querySelector('.icon-overlay svg.icon')).not.toBeNull();
		const fill = el!.querySelector('svg.ring .ring-fill');
		const circ = Number(fill!.getAttribute('stroke-dasharray'));
		const off = Number(fill!.getAttribute('stroke-dashoffset'));
		// 50% -> dashoffset is half the circumference.
		expect(off).toBeCloseTo(circ * 0.5, 1);
	});

	it('renders a fully-filled ring at 100% brightness', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BrightnessIndicator, { props: { client } });
		await emit({
			available: true,
			displays: [
				{
					subsystem: 'backlight',
					name: 'intel_backlight',
					current: 10000,
					max: 10000,
				},
			],
		});
		const fill = container.querySelector('svg.ring .ring-fill');
		const off = Number(fill!.getAttribute('stroke-dashoffset'));
		expect(off).toBeCloseTo(0, 1);
	});

	it('scroll up invokes adjust with delta_percent 5 and first-display fields', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BrightnessIndicator, { props: { client } });
		await emit({
			available: true,
			displays: [
				{
					subsystem: 'backlight',
					name: 'intel_backlight',
					current: 5000,
					max: 10000,
				},
			],
		});
		const el = container.querySelector('.bar-button');
		expect(el).not.toBeNull();
		fireEvent.wheel(el!, { deltaY: -10 });
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'brightness',
			action: {
				kind: 'custom',
				data: {
					kind: 'brightness',
					payload: {
						command: 'adjust',
						subsystem: 'backlight',
						name: 'intel_backlight',
						delta_percent: 5,
					},
				},
			},
		});
	});

	it('scroll down invokes adjust with delta_percent -5', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BrightnessIndicator, { props: { client } });
		await emit({
			available: true,
			displays: [
				{
					subsystem: 'backlight',
					name: 'intel_backlight',
					current: 5000,
					max: 10000,
				},
			],
		});
		const el = container.querySelector('.bar-button');
		expect(el).not.toBeNull();
		fireEvent.wheel(el!, { deltaY: 10 });
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'brightness',
			action: {
				kind: 'custom',
				data: {
					kind: 'brightness',
					payload: {
						command: 'adjust',
						subsystem: 'backlight',
						name: 'intel_backlight',
						delta_percent: -5,
					},
				},
			},
		});
	});

	it('tooltip includes subsystem/name and percent', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BrightnessIndicator, { props: { client } });
		await emit({
			available: true,
			displays: [
				{
					subsystem: 'backlight',
					name: 'intel_backlight',
					current: 5000,
					max: 10000,
				},
			],
		});
		const el = container.querySelector('.bar-button');
		expect(el).not.toBeNull();
		expect(el!.title).toContain('backlight/intel_backlight');
		expect(el!.title).toContain('50%');
	});

	it('sets the backlight to a preset percent from the right-click menu', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BrightnessIndicator, { props: { client } });
		await emit({
			available: true,
			displays: [
				{
					subsystem: 'backlight',
					name: 'intel_backlight',
					current: 2000,
					max: 10000,
				},
			],
		});

		const el = container.querySelector('.bar-button') as HTMLButtonElement;
		await fireEvent.contextMenu(el);
		await tick();

		const fifty = menuItem('50%');
		expect(fifty).toBeTruthy();
		await fireEvent.click(fifty as HTMLButtonElement);
		await tick();

		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'brightness',
			action: {
				kind: 'custom',
				data: {
					kind: 'brightness',
					payload: {
						command: 'set',
						subsystem: 'backlight',
						name: 'intel_backlight',
						value: Math.round(0.5 * 10000),
					},
				},
			},
		});
	});
});
