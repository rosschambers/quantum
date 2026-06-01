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

    /**
     * Unicode bluetooth symbol U+1F542 isn't actually a thing; the
     * de-facto standard for "looks like bluetooth" is U+1F4F6 (signal
     * bars) is for wifi. The bluetooth glyph is U+1F4F1? No \u2014
     * the actual codepoint is U+1F4F6/U+1F4F1 for signals/phone.
     * The closest plain-text bluetooth symbol is U+1F4DC? No.
     * The standard is U+2733 ('EIGHT POINTED PINWHEEL STAR') doesn't
     * look right either. Best universally-rendered approximation is
     * the chain link U+1F517 or simply the letter B in a circle.
     * We use 'BT' as a compact textual indicator and rely on color +
     * the device count for state.
     */
    function iconFor(s: BluetoothState): string {
        if (!s.powered) return 'BT';
        if (s.discovering) return 'BT\u2026';
        return 'BT';
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
    <div
        bind:this={root}
        class="tray-icon bluetooth"
        class:powered={state.powered}
        class:has-devices={state.connected_devices.length > 0}
        title={tooltipFor(state)}
    >
        <span class="icon" aria-hidden="true">{iconFor(state)}</span>
        {#if state.connected_devices.length > 0}
            <span class="badge">{state.connected_devices.length}</span>
        {/if}
    </div>
{/if}

<style>
    .tray-icon {
        display: inline-flex;
        align-items: center;
        gap: 2px;
        font-size: var(--tray-icon-size, 14px);
        color: var(--tray-icon-color, var(--color-fg-alt, #a6adc8));
        padding: 0 4px;
        user-select: none;
        cursor: pointer;
        line-height: 1;
    }
    .tray-icon.powered { color: var(--tray-icon-color-active, var(--color-accent, #89b4fa)); }
    .icon {
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: calc(var(--tray-icon-size, 14px) - 2px);
        font-weight: 600;
    }
    .badge {
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 9px;
        background: var(--color-bg-alt, #313244);
        padding: 1px 3px;
        border-radius: 999px;
        line-height: 1;
    }
</style>
