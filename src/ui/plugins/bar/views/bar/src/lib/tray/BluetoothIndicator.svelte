<script lang="ts">
    import type { Client, MenuItem } from '@quantum/client';
    import type { BluetoothState } from '../types';
    import { BLUETOOTH_CHANNEL, BLUETOOTH_PROVIDER } from '../channels';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';
    import { monitorView, wireBarMenu } from './barMenu';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: BluetoothState = $state({
        available: false,
        powered: false,
        discovering: false,
        devices: [],
        adapter_path: '',
    });
    let buttonEl: HTMLButtonElement | undefined = $state(undefined);

    const connectedDevices = $derived(state.devices.filter((device) => device.connected));

    $effect(() => {
        client
            .call('provider.query', { id: BLUETOOTH_PROVIDER })
            .then((r: unknown) => {
                if (r) state = r as BluetoothState;
            })
            .catch(() => {});
        const unsubscribe = client.subscribe(BLUETOOTH_CHANNEL, (payload: unknown) => {
            // Pairing events share the channel; only state payloads (which
            // never carry an `event` key) may replace indicator state.
            if ((payload as { event?: string }).event !== undefined) return;
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
        for (const device of connectedDevices) {
            items.push({
                label: `Disconnect ${device.name}`,
                onSelect: () =>
                    invoke({ command: 'disconnect', address: device.address }).catch((err) =>
                        console.error('bluetooth disconnect failed:', err),
                    ),
            });
        }
        items.push({ separator: true, label: '' });
        items.push({ label: 'Open Bluetooth manager', onSelect: () => void openBluetoothManager() });
        return items;
    }

    async function openBluetoothManager(): Promise<void> {
        // The bar widget is injected with a per-monitor `__quantum_monitor`
        // so the window opens on the same display as the bar that was clicked.
        const name = monitorView('plugin/bluetooth-menu/bluetooth-menu');
        try {
            await client.call('view.show', { name });
        } catch (err) {
            console.error(`view.show ${name} failed:`, err);
        }
        // Kick the discovery session from the bar: the overlay's webview may
        // be served warm across hide/show, in which case its mount effect
        // does not re-run (same pattern as NetworkIndicator.openWifiMenu).
        invoke({ command: 'open_session' }).catch((err) =>
            console.error('bluetooth open_session failed:', err),
        );
    }

    function tooltipFor(devices: BluetoothState['devices']): string {
        if (!state.powered) return 'bluetooth off';
        if (devices.length === 0) return 'bluetooth on, no devices';
        const names = devices
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
        title={tooltipFor(connectedDevices)}
        onclick={openBluetoothManager}
        bindRef={(el) => (buttonEl = el)}
    >
        <span
            class="bluetooth-icon"
            class:powered={state.powered}
            class:has-devices={connectedDevices.length > 0}
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
