<script lang="ts">
	import type { Client } from '@quantum/client';
	import type { NetworkState } from '../types';
	import { NETWORK_CHANNEL } from '../channels';
	import { onClick } from './interaction';

	interface Props {
		client: Client;
	}

	let { client }: Props = $props();
	let state: NetworkState = $state({
		available: false,
		connectivity: 'unknown',
		primary: null,
		wifi_enabled: false,
		wifi_signal_percent: null,
	});
	let root: HTMLElement | undefined = $state(undefined);

	$effect(() => {
		client.call('provider.query', { id: 'network' })
			.then((r: unknown) => { if (r) state = r as NetworkState; })
			.catch(() => {});
		const unsubscribe = client.subscribe(NETWORK_CHANNEL, (payload: unknown) => {
			state = payload as NetworkState;
		});
		return () => unsubscribe?.();
	});

	$effect(() => {
		if (!root) return;
		const off1 = onClick(root, toggleWifi, 'left');
		// Right click reserved for the connection-list popover (deferred).
		const off2 = onClick(root, openPopover, 'right');
		return () => {
			off1();
			off2();
		};
	});

	async function toggleWifi(): Promise<void> {
		if (!state.available) return;
		try {
			await client.call('action.invoke', {
				provider: 'network',
				action: {
					kind: 'custom',
					data: {
						kind: 'network',
						payload: { command: 'set_wifi_enabled', value: !state.wifi_enabled },
					},
				},
			});
		} catch (err) {
			console.error('network toggle failed:', err);
		}
	}

	function openPopover(): void {
		// TODO: connection-list popover. Deferred from batch 1.
	}

	function iconFor(s: NetworkState): string {
		if (!s.available) return '';
		if (!s.primary) return s.wifi_enabled ? '○' : '×';
		switch (s.primary.kind) {
			case 'ethernet':
				return '⊞';
			case 'wifi': {
				const sig = s.wifi_signal_percent ?? 0;
				if (sig >= 75) return '▮▮▮';
				if (sig >= 50) return '▮▮▯';
				if (sig >= 25) return '▮▯▯';
				return '▯▯▯';
			}
			case 'cellular':
				return '◫';
			case 'vpn':
				return '⇄';
			default:
				return '·';
		}
	}

	function tooltipFor(s: NetworkState): string {
		if (!s.available) return 'network unavailable';
		if (!s.primary) return s.wifi_enabled ? 'wifi on, no connection' : 'wifi off';
		const parts = [s.primary.id];
		if (s.primary.ssid) parts.push(s.primary.ssid);
		if (s.wifi_signal_percent !== null) parts.push(`${s.wifi_signal_percent}%`);
		return parts.join(' · ');
	}
</script>

{#if state.available}
	<div bind:this={root} class="tray-icon network" title={tooltipFor(state)}>
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
