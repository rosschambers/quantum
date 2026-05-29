import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import VolumeIndicator from './VolumeIndicator.svelte';
import type { AudioState } from '../types';

function mockClient(): {
	client: any;
	emit: (p: AudioState) => Promise<void>;
} {
	let saved: ((p: unknown) => void) | undefined;
	const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
		saved = cb;
		return () => {};
	});
	const call = vi.fn();
	const client = { call, subscribe, close: vi.fn() };
	const emit = async (p: AudioState) => {
		await tick();
		saved?.(p);
		await tick();
	};
	return { client, emit };
}

describe('VolumeIndicator', () => {
	it('renders nothing when unavailable', () => {
		const { client } = mockClient();
		const { container } = render(VolumeIndicator, { props: { client } });
		expect(container.querySelector('.tray-icon')).toBeNull();
	});

	it('renders nothing when default_sink is null even if available', async () => {
		const { client, emit } = mockClient();
		const { container } = render(VolumeIndicator, { props: { client } });
		await emit({
			available: true,
			default_sink: null,
		});
		expect(container.querySelector('.tray-icon')).toBeNull();
	});

	it('renders mid-volume glyph at 50%', async () => {
		const { client, emit } = mockClient();
		const { container } = render(VolumeIndicator, { props: { client } });
		await emit({
			available: true,
			default_sink: {
				name: 's',
				description: 'Built-in',
				volume_percent: 50,
				muted: false,
			},
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.textContent).toContain('▮▮▯');
	});

	it('renders muted glyph when muted', async () => {
		const { client, emit } = mockClient();
		const { container } = render(VolumeIndicator, { props: { client } });
		await emit({
			available: true,
			default_sink: {
				name: 's',
				description: 'Built-in',
				volume_percent: 50,
				muted: true,
			},
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.textContent).toContain('⊘');
	});

	it('left click invokes toggle_mute', async () => {
		const { client, emit } = mockClient();
		const { container } = render(VolumeIndicator, { props: { client } });
		await emit({
			available: true,
			default_sink: {
				name: 's',
				description: 'Built-in',
				volume_percent: 50,
				muted: false,
			},
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.click(el!);
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'audio',
			action: {
				kind: 'custom',
				data: {
					kind: 'audio',
					payload: { command: 'toggle_mute' },
				},
			},
		});
	});

	it('scroll up invokes adjust_volume with delta 5', async () => {
		const { client, emit } = mockClient();
		const { container } = render(VolumeIndicator, { props: { client } });
		await emit({
			available: true,
			default_sink: {
				name: 's',
				description: 'Built-in',
				volume_percent: 50,
				muted: false,
			},
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.wheel(el!, { deltaY: -10 });
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'audio',
			action: {
				kind: 'custom',
				data: {
					kind: 'audio',
					payload: { command: 'adjust_volume', delta: 5 },
				},
			},
		});
	});

	it('scroll down invokes adjust_volume with delta -5', async () => {
		const { client, emit } = mockClient();
		const { container } = render(VolumeIndicator, { props: { client } });
		await emit({
			available: true,
			default_sink: {
				name: 's',
				description: 'Built-in',
				volume_percent: 50,
				muted: false,
			},
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		fireEvent.wheel(el!, { deltaY: 10 });
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'audio',
			action: {
				kind: 'custom',
				data: {
					kind: 'audio',
					payload: { command: 'adjust_volume', delta: -5 },
				},
			},
		});
	});

	it('tooltip includes description and percent', async () => {
		const { client, emit } = mockClient();
		const { container } = render(VolumeIndicator, { props: { client } });
		await emit({
			available: true,
			default_sink: {
				name: 's',
				description: 'Built-in',
				volume_percent: 50,
				muted: false,
			},
		});
		const el = container.querySelector('.tray-icon');
		expect(el).not.toBeNull();
		expect(el!.title).toContain('Built-in');
		expect(el!.title).toContain('50%');
	});
});
