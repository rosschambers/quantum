<script lang="ts">
	import type { Client } from '@quantum/client';
	import type { BrightnessState } from '../types';
	import { BRIGHTNESS_CHANNEL } from '../channels';
	import { onScroll } from './interaction';

	interface Props {
		client: Client;
	}

	let { client }: Props = $props();
	let state: BrightnessState = $state({
		available: false,
		displays: [],
	});
	let root: HTMLElement | undefined = $state(undefined);

	$effect(() => {
		client.call('provider.query', { id: 'brightness' })
			.then((r: unknown) => { if (r) state = r as BrightnessState; })
			.catch(() => {});
		const unsubscribe = client.subscribe(BRIGHTNESS_CHANNEL, (payload: unknown) => {
			state = payload as BrightnessState;
		});
		return () => unsubscribe?.();
	});

	$effect(() => {
		if (!root) return;
		const off = onScroll(root, handleScroll);
		return () => {
			off();
		};
	});

	async function handleScroll(delta: 1 | -1): Promise<void> {
		if (!state.available || state.displays.length === 0) return;
		const display = state.displays[0];
		const deltaPercent = delta === 1 ? 5 : -5;
		try {
			await client.call('action.invoke', {
				provider: 'brightness',
				action: {
					kind: 'custom',
					data: {
						kind: 'brightness',
						payload: {
							command: 'adjust',
							subsystem: display.subsystem,
							name: display.name,
							delta_percent: deltaPercent,
						},
					},
				},
			});
		} catch (err) {
			console.error('brightness adjust failed:', err);
		}
	}

	function iconFor(s: BrightnessState): string {
		if (s.displays.length === 0) return '';
		const display = s.displays[0];
		const bucket = (display.current / display.max) * 100;
		if (bucket < 33) return '▮▯▯';
		if (bucket < 67) return '▮▮▯';
		return '▮▮▮';
	}

	function tooltipFor(s: BrightnessState): string {
		if (s.displays.length === 0) return 'brightness unavailable';
		const display = s.displays[0];
		const pct = Math.round((display.current / display.max) * 100);
		return `${display.subsystem}/${display.name} · ${pct}%`;
	}
</script>

{#if state.available && state.displays.length > 0}
	<div bind:this={root} class="tray-icon brightness" title={tooltipFor(state)}>
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
