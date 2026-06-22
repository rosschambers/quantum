<script lang="ts">
    import type { Client, MenuItem } from '@quantum/client';
    import type { BluetoothState } from '../types';
    import { BLUETOOTH_CHANNEL, BLUETOOTH_PROVIDER } from '../channels';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';
    import { wireBarMenu } from './barMenu';

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
    let buttonEl: HTMLButtonElement | undefined = $state(undefined);

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

    // Right-click opens quick Bluetooth actions built from the current radio
    // state and connected devices.
    $effect(() => {
        const node = buttonEl;
        if (!node) return;
        return wireBarMenu(node, client, buildMenuItems);
    });

    function invoke(payload: Record<string, unknown>): Promise<unknown> {
        return client.call('action.invoke', {
            provider: 'bluetooth',
            action: {
                kind: 'custom',
                data: { kind: 'bluetooth', payload },
            },
        });
    }

    function buildMenuItems(): MenuItem[] {
        const items: MenuItem[] = [];
        const powered = state.powered;
        items.push({
            label: `Turn Bluetooth ${powered ? 'off' : 'on'}`,
            onSelect: () =>
                invoke({ command: 'set_powered', value: !powered }).catch((err) =>
                    console.error('bluetooth set_powered failed:', err),
                ),
        });
        for (const device of state.connected_devices) {
            items.push({
                label: `Disconnect ${device.name}`,
                onSelect: () =>
                    invoke({ command: 'disconnect', address: device.address }).catch((err) =>
                        console.error('bluetooth disconnect failed:', err),
                    ),
            });
        }
        items.push({ separator: true, label: '' });
        items.push({ label: 'Open Bluetooth manager', onSelect: launchBluemanManager });
        return items;
    }

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
        bindRef={(el) => (buttonEl = el)}
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
