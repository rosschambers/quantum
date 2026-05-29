<script lang="ts">
	import type { Client } from '@quantum/client';
	import type { AudioState } from '../types';
	import { AUDIO_CHANNEL } from '../channels';
	import { onClick, onScroll } from './interaction';

	interface Props {
		client: Client;
	}

	let { client }: Props = $props();
	let state: AudioState = $state({
		available: false,
		default_sink: null,
	});
	let root: HTMLElement | undefined = $state(undefined);

	$effect(() => {
		const unsubscribe = client.subscribe(AUDIO_CHANNEL, (payload: unknown) => {
			state = payload as AudioState;
		});
		return () => unsubscribe?.();
	});

	$effect(() => {
		if (!root) return;
		const off1 = onClick(root, handleClick, 'left');
		const off2 = onScroll(root, handleScroll);
		return () => {
			off1();
			off2();
		};
	});

	async function handleClick(): Promise<void> {
		if (!state.available || !state.default_sink) return;
		try {
			await client.call('action.invoke', {
				provider: 'audio',
				action: {
					kind: 'custom',
					data: {
						kind: 'audio',
						payload: { command: 'toggle_mute' },
					},
				},
			});
		} catch (err) {
			console.error('audio toggle_mute failed:', err);
		}
	}

	async function handleScroll(delta: 1 | -1): Promise<void> {
		if (!state.available || !state.default_sink) return;
		const volumeDelta = delta === 1 ? 5 : -5;
		try {
			await client.call('action.invoke', {
				provider: 'audio',
				action: {
					kind: 'custom',
					data: {
						kind: 'audio',
						payload: { command: 'adjust_volume', delta: volumeDelta },
					},
				},
			});
		} catch (err) {
			console.error('audio adjust_volume failed:', err);
		}
	}

	function iconFor(s: AudioState): string {
		if (!s.default_sink) return '';
		if (s.default_sink.muted) return '⊘';
		const v = s.default_sink.volume_percent;
		if (v === 0) return '⊘';
		if (v < 33) return '▮▯▯';
		if (v < 67) return '▮▮▯';
		return '▮▮▮';
	}

	function tooltipFor(s: AudioState): string {
		if (!s.default_sink) return 'audio unavailable';
		const muted = s.default_sink.muted ? ' (muted)' : '';
		return `${s.default_sink.description} · ${s.default_sink.volume_percent}%${muted}`;
	}
</script>

{#if state.available && state.default_sink !== null}
	<div bind:this={root} class="tray-icon volume" title={tooltipFor(state)}>
		{iconFor(state)}
	</div>
{/if}

<style>
	.tray-icon {
		font-size: var(--tray-icon-size, 14px);
		color: var(--tray-icon-color, var(--color-fg, #cdd6f4));
		padding: 0 4px;
		user-select: none;
		cursor: pointer;
	}
</style>
