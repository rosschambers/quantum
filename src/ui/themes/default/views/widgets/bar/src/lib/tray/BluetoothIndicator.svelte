<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { BluetoothState } from '../types';
    import { BLUETOOTH_CHANNEL, BLUETOOTH_PROVIDER } from '../channels';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';

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

    $effect(() => {
        client
            .call('provider.query', { id: BLUETOOTH_PROVIDER })
            .then((r: unknown) => {
                if (r) state = r as BluetoothState;
            })
            .catch(() => {});
        const unsubscribe = client.subscribe(BLUETOOTH_CHANNEL, (payload: unknown) => {
            state = payload as BluetoothState;
        });
        return () => unsubscribe?.();
    });

    async function launchBluemanManager(): Promise<void> {
        try {
            await client.call('action.invoke', {
                provider: 'shell',
                action: {
                    kind: 'shell',
                    data: { command: ['blueman-manager'], terminal: false },
                },
            });
        } catch (err) {
            console.error('blueman-manager launch failed:', err);
        }
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
    <BarButton
        ariaLabel="Bluetooth"
        title={tooltipFor(state)}
        onclick={launchBluemanManager}
    >
        <span
            class="bluetooth-icon"
            class:powered={state.powered}
            class:has-devices={state.connected_devices.length > 0}
        >
            <Icon name="bluetooth" size={18} />
        </span>
    </BarButton>
{/if}

<style>
    .bluetooth-icon {
        display: inline-flex;
        align-items: center;
        line-height: 1;
    }
    /*
     * Powered + at least one connected device shifts the icon to the
     * accent color; powered + no devices keeps it muted (still tells
     * the user the radio is on but reads less prominent than active
     * connections); off keeps it muted too.
     */
    .bluetooth-icon.powered.has-devices {
        color: var(--color-accent, #89b4fa);
    }
</style>
