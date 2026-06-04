<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { NetworkState } from '../types';
    import { NETWORK_CHANNEL, NETWORK_PROVIDER } from '../channels';
    import { inverseGradientColor } from '../gradient';
    import { networkIcon } from '../icons';
    import { onClick } from './interaction';
    import Ring from '../Ring.svelte';

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
        client
            .call('provider.query', { id: NETWORK_PROVIDER })
            .then((r: unknown) => {
                if (r) state = r as NetworkState;
            })
            .catch(() => {});
        const unsubscribe = client.subscribe(NETWORK_CHANNEL, (payload: unknown) => {
            state = payload as NetworkState;
        });
        return () => unsubscribe?.();
    });

    $effect(() => {
        if (!root) return;
        const off1 = onClick(root, toggleWifi, 'left');
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

    function ringPercent(s: NetworkState): number | null {
        if (!s.available || !s.primary) return null;
        if (s.primary.kind === 'wifi' || s.primary.kind === 'cellular') {
            return s.wifi_signal_percent;
        }
        return 100;
    }

    function tooltipFor(s: NetworkState): string {
        if (!s.available) return 'network unavailable';
        if (!s.primary) return s.wifi_enabled ? 'wifi on, no connection' : 'wifi off';
        const parts = [s.primary.id];
        if (s.primary.ssid) parts.push(s.primary.ssid);
        if (s.wifi_signal_percent !== null) parts.push(`${s.wifi_signal_percent}%`);
        return parts.join(' \u00b7 ');
    }
</script>

{#if state.available}
    <div bind:this={root} class="tray-icon network" title={tooltipFor(state)}>
        <Ring
            percent={ringPercent(state)}
            color={inverseGradientColor(ringPercent(state))}
            kind="icon"
            iconName={networkIcon(state.primary?.kind ?? null, state.primary !== null)}
        />
    </div>
{/if}

<style>
    .tray-icon {
        display: inline-flex;
        align-items: center;
        color: var(--tray-icon-color, var(--color-fg, #cdd6f4));
        user-select: none;
        cursor: pointer;
        line-height: 1;
    }
</style>
