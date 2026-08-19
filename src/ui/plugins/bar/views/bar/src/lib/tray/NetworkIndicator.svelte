<script lang="ts">
    import type { Client, MenuItem } from '@quantum/client';
    import type { NetworkState } from '../types';
    import { NETWORK_CHANNEL, NETWORK_PROVIDER, WIFI_PROVIDER } from '../channels';
    import { networkIcon } from '../icons';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';
    import { monitorView, wireBarMenu } from './barMenu';

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
    let buttonEl: HTMLButtonElement | undefined = $state(undefined);

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

    // Right-click toggles the Wi-Fi radio (network provider, kind 'network')
    // and links to the full Wi-Fi settings overlay.
    $effect(() => {
        const node = buttonEl;
        if (!node) return;
        return wireBarMenu(node, client, buildMenuItems);
    });

    function buildMenuItems(): MenuItem[] {
        const enabled = state.wifi_enabled;
        return [
            {
                label: `Turn Wi-Fi ${enabled ? 'off' : 'on'}`,
                onSelect: () =>
                    client
                        .call('action.invoke', {
                            provider: 'network',
                            action: {
                                kind: 'custom',
                                data: {
                                    kind: 'network',
                                    payload: { command: 'set_wifi_enabled', value: !enabled },
                                },
                            },
                        })
                        .catch((err) => console.error('network set_wifi_enabled failed:', err)),
            },
            { separator: true, label: '' },
            { label: 'Wi-Fi settings...', onSelect: openWifiMenu },
        ];
    }

    async function openWifiMenu(): Promise<void> {
        // The bar widget is injected with a per-monitor `__quantum_monitor`
        // so the menu opens on the same display as the bar that was clicked.
        // When unknown, the bare name lets the daemon place it on the focused
        // monitor.
        const name = monitorView('plugin/wifi-menu/wifi-menu');
        try {
            await client.call('view.show', { name });
        } catch (err) {
            console.error(`view.show ${name} failed:`, err);
        }
        // Start the Wi-Fi provider's scan session. The overlay's webview is
        // kept warm across hide/show, so on a re-open it does not re-mount to
        // start scanning itself; the bar (which just showed it) kicks the
        // session so the network list is live. The overlay stops the session
        // when it is dismissed.
        client
            .call('action.invoke', {
                provider: WIFI_PROVIDER,
                action: {
                    kind: 'custom',
                    data: { kind: 'wifi', payload: { command: 'open_session' } },
                },
            })
            .catch((err) => console.error('wifi open_session failed:', err));
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
        bindRef={(el) => (buttonEl = el)}
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
            <!-- signal percentage shown in wifi-menu overlay only -->
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
