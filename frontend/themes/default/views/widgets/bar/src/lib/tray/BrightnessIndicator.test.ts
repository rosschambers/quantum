import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import BrightnessIndicator from './BrightnessIndicator.svelte';
import type { BrightnessState } from '../types';

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
		expect(container.querySelector('.tray-icon')).toBeNull();
	});

	it('renders nothing when displays array is empty even if available', async () => {
		const { client, emit } = mockClient();
		const { container } = render(BrightnessIndicator, { props: { client } });
		await emit({
			available: true,
			displays: [],
		});
		expect(container.querySelector('.tray-icon')).toBeNull();
	});

	it('renders mid glyph at 50% brightness', async () => {
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.textContent).toContain('▮▮▯');
	});

	it('renders high glyph at 100% brightness', async () => {
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.textContent).toContain('▮▮▮');
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
		const el = container.querySelector('.tray-icon');
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
		const el = container.querySelector('.tray-icon');
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
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.title).toContain('backlight/intel_backlight');
		expect(el!.title).toContain('50%');
	});
});
