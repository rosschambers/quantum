<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { NetworkState } from '../types';
    import { NETWORK_CHANNEL } from '../channels';
    import { gradientColor } from '../gradient';
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
            .call('provider.query', { id: 'network' })
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

    /**
     * Choose a glyph for the active connection type. Wifi/cellular get
     * the wireless signal glyph; ethernet gets a plug glyph; vpn gets a
     * shield. Each ends with U+FE0E so emoji-presentation codepoints
     * render in the bar's text color rather than full-color emoji.
     */
    function iconFor(s: NetworkState): string {
        if (!s.available) return '\u{1f6c8}\u{fe0e}';
        if (!s.primary) return s.wifi_enabled ? '\u{1f4f6}\u{fe0e}' : '\u2715\u{fe0e}';
        switch (s.primary.kind) {
            case 'ethernet': return '\u{1f50c}\u{fe0e}';
            case 'wifi': return '\u{1f4f6}\u{fe0e}';
            case 'cellular': return '\u{1f4f6}\u{fe0e}';
            case 'vpn': return '\u{1f512}\u{fe0e}';
            default: return '\u{1f310}\u{fe0e}';
        }
    }

    /**
     * Percentage that drives the ring. Wifi/cellular use signal
     * strength. Ethernet uses 100 when connected (the ring is just a
     * visual confirmation of an active link). No primary connection
     * returns null so the ring renders empty.
     */
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
        <span class="icon" aria-hidden="true">{iconFor(state)}</span>
        <Ring percent={ringPercent(state)} color={gradientColor(ringPercent(state))} />
    </div>
{/if}

<style>
    .tray-icon {
        display: inline-flex;
        align-items: center;
        gap: 3px;
        font-size: var(--tray-icon-size, 14px);
        color: var(--tray-icon-color, var(--color-fg, #cdd6f4));
        padding: 0 4px;
        user-select: none;
        cursor: pointer;
        line-height: 1;
    }
    .icon {
        font-size: var(--tray-icon-size, 14px);
        line-height: 1;
        font-variant-emoji: text;
    }
</style>
