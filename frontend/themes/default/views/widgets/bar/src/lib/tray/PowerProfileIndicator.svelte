<script lang="ts">
	import type { Client } from '@quantum/client';
	import type { PowerProfile, PowerProfileState } from '../types';
	import { POWER_PROFILE_CHANNEL } from '../channels';
	import { onClick } from './interaction';

	interface Props {
		client: Client;
	}

	let { client }: Props = $props();
	let state: PowerProfileState = $state({
		available: false,
		active: null,
		profiles: [],
		performance_inhibited: null,
	});
	let root: HTMLElement | undefined = $state(undefined);

	$effect(() => {
		client.call('provider.query', { id: 'power_profile' })
			.then((r: unknown) => { if (r) state = r as PowerProfileState; })
			.catch(() => {});
		const unsubscribe = client.subscribe(POWER_PROFILE_CHANNEL, (payload: unknown) => {
			state = payload as PowerProfileState;
		});
		return () => unsubscribe?.();
	});

	$effect(() => {
		if (!root) return;
		const off = onClick(root, cycleProfile, 'left');
		return () => {
			off();
		};
	});

	async function cycleProfile(): Promise<void> {
		if (!state.available || !state.active) return;
		const next = nextProfile(state);
		if (!next) return;
		try {
			await client.call('action.invoke', {
				provider: 'power_profile',
				action: {
					kind: 'custom',
					data: {
						kind: 'power_profile',
						payload: { command: 'set', profile: next },
					},
				},
			});
		} catch (err) {
			console.error('power profile set failed:', err);
		}
	}

	function nextProfile(s: PowerProfileState): PowerProfile | null {
		if (!s.active) return null;
		const order: PowerProfile[] = ['power_saver', 'balanced', 'performance'];
		const available = order.filter(
			(p) => s.profiles.includes(p) && !(p === 'performance' && s.performance_inhibited !== null),
		);
		if (available.length === 0) return null;
		const idx = available.indexOf(s.active);
		if (idx === -1) return available[0];
		return available[(idx + 1) % available.length];
	}

	function iconFor(s: PowerProfileState): string {
		switch (s.active) {
			case 'power_saver':
				return '◯';
			case 'balanced':
				return '◐';
			case 'performance':
				return '●';
			default:
				return '';
		}
	}

	function tooltipFor(s: PowerProfileState): string {
		if (!s.active) return 'power profile unavailable';
		let t = s.active;
		if (s.performance_inhibited) t += ` (perf inhibited: ${s.performance_inhibited})`;
		return t;
	}
</script>

{#if state.available && state.active !== null}
	<div bind:this={root} class="tray-icon power-profile" title={tooltipFor(state)}>
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
