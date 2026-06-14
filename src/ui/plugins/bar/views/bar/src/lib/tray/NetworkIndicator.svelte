<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { NetworkState } from '../types';
    import { NETWORK_CHANNEL, NETWORK_PROVIDER } from '../channels';
    import { networkIcon } from '../icons';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';

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

    async function openWifiMenu(): Promise<void> {
        try {
            await client.call('view.show', { name: 'widgets/wifi-menu' });
        } catch (err) {
            console.error('open wifi menu failed:', err);
        }
    }

    function tooltipFor(s: NetworkState): string {
        if (!s.primary) {
            const base = s.wifi_enabled ? 'wifi on, no connection' : 'wifi off';
            return `${base} \u00b7 ${s.connectivity}`;
        }
        const name = s.primary.ssid ?? s.primary.id;
        return `${name} \u00b7 ${s.connectivity}`;
    }
</script>

{#if state.available}
    <BarButton
        ariaLabel="Network"
        title={tooltipFor(state)}
        onclick={openWifiMenu}
    >
        <span
            class="network-icon"
            class:connected={state.primary !== null}
            class:full={state.connectivity === 'full'}
        >
            <Icon
                name={networkIcon(state.primary?.kind ?? null, state.primary !== null)}
                size={18}
            />
        </span>
    </BarButton>
{/if}

<style>
    .network-icon {
        display: inline-flex;
        align-items: center;
        line-height: 1;
    }
    /*
     * A primary connection with full connectivity shifts the icon to
     * the accent color. A connection with limited/portal connectivity
     * keeps the icon muted so the user can tell the link is up but
     * not actually reaching the wider network. No connection keeps it
     * muted too.
     */
    .network-icon.connected.full {
        color: var(--color-accent, #89b4fa);
    }
</style>
