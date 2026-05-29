<script lang="ts">
	import type { Client } from '@quantum/client';
	import type { BluetoothState } from '../types';
	import { BLUETOOTH_CHANNEL } from '../channels';
	import { onClick } from './interaction';

	interface Props {
		client: Client;
	}

	let { client }: Props = $props();
	let state: BluetoothState = $state({
		available: false,
		powered: false,
		discovering: false,
		connected_devices: [],
	});
	let root: HTMLElement | undefined = $state(undefined);

	$effect(() => {
		const unsubscribe = client.subscribe(BLUETOOTH_CHANNEL, (payload: unknown) => {
			state = payload as BluetoothState;
		});
		return () => unsubscribe?.();
	});

	$effect(() => {
		if (!root) return;
		const off1 = onClick(root, togglePowered, 'left');
		// Right click reserved for the device-list popover (deferred).
		const off2 = onClick(root, openPopover, 'right');
		return () => {
			off1();
			off2();
		};
	});

	async function togglePowered(): Promise<void> {
		if (!state.available) return;
		try {
			await client.call('action.invoke', {
				provider: 'bluetooth',
				action: {
					kind: 'custom',
					data: {
						kind: 'bluetooth',
						payload: { command: 'set_powered', value: !state.powered },
					},
				},
			});
		} catch (err) {
			console.error('bluetooth toggle failed:', err);
		}
	}

	function openPopover(): void {
		// TODO: device-list popover. Deferred from batch 1.
	}

	function iconFor(s: BluetoothState): string {
		if (!s.powered) return '⊘';
		if (s.discovering) return '⊙';
		const count = s.connected_devices.length;
		return count === 0 ? '⊕' : `⊕${count}`;
	}

	function tooltipFor(s: BluetoothState): string {
		if (!s.powered) return 'bluetooth off';
		if (s.connected_devices.length === 0) return 'bluetooth on, no devices';
		const names = s.connected_devices
			.map((dev) => {
				const battery = dev.battery_percent !== null ? ` (${dev.battery_percent}%)` : '';
				return `${dev.name}${battery}`;
			})
			.join(', ');
		return names;
	}
</script>

{#if state.available}
	<div bind:this={root} class="tray-icon bluetooth" title={tooltipFor(state)}>
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
