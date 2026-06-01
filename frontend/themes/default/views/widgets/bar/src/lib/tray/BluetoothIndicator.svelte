<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { BluetoothState } from '../types';
    import { BLUETOOTH_CHANNEL } from '../channels';
    import Icon from '../Icon.svelte';
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
        client
            .call('provider.query', { id: 'bluetooth' })
            .then((r: unknown) => {
                if (r) state = r as BluetoothState;
            })
            .catch(() => {});
        const unsubscribe = client.subscribe(BLUETOOTH_CHANNEL, (payload: unknown) => {
            state = payload as BluetoothState;
        });
        return () => unsubscribe?.();
    });

    $effect(() => {
        if (!root) return;
        const off1 = onClick(root, togglePowered, 'left');
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

    function tooltipFor(s: BluetoothState): string {
        if (!s.powered) return 'bluetooth off';
        if (s.connected_devices.length === 0) return 'bluetooth on, no devices';
        const names = s.connected_devices
            .map((dev) => {
                const battery =
                    dev.battery_percent !== null ? ` (${dev.battery_percent}%)` : '';
                return `${dev.name}${battery}`;
            })
            .join(', ');
        return names;
    }
</script>

{#if state.available}
    <div
        bind:this={root}
        class="tray-icon bluetooth"
        class:powered={state.powered}
        class:has-devices={state.connected_devices.length > 0}
        title={tooltipFor(state)}
    >
        <Icon name="bluetooth" size={14} />
        {#if state.connected_devices.length > 0}
            <span class="badge">{state.connected_devices.length}</span>
        {/if}
    </div>
{/if}

<style>
    .tray-icon {
        display: inline-flex;
        align-items: center;
        gap: 3px;
        color: var(--color-fg-alt, #a6adc8);
        user-select: none;
        cursor: pointer;
        line-height: 1;
    }
    /*
     * Powered + at least one connected device shifts the icon to the
     * accent color; powered + no devices keeps it muted (still tells
     * the user the radio is on but reads less prominent than active
     * connections); off keeps it muted too.
     */
    .tray-icon.powered.has-devices {
        color: var(--color-accent, #89b4fa);
    }

    .badge {
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 9px;
        background: var(--color-bg-alt, #313244);
        padding: 1px 3px;
        border-radius: 999px;
        line-height: 1;
        color: var(--color-fg, #cdd6f4);
    }
</style>
