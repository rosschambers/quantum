<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { PowerState } from '../types';
    import { POWER_CHANNEL } from '../channels';
    import { gradientColor } from '../gradient';
    import { batteryIcon } from '../icons';
    import Ring from '../Ring.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: PowerState = $state({
        available: false,
        on_battery: false,
        percentage: null,
        state: null,
        time_to_empty_secs: null,
        time_to_full_secs: null,
    });

    $effect(() => {
        client
            .call('provider.query', { id: 'power' })
            .then((r: unknown) => {
                if (r) state = r as PowerState;
            })
            .catch(() => {});
        const unsubscribe = client.subscribe(POWER_CHANNEL, (payload: unknown) => {
            state = payload as PowerState;
        });
        return () => unsubscribe?.();
    });

    function tooltipFor(s: PowerState): string {
        if (!s.available) return 'battery unavailable';
        const parts: string[] = [];
        if (s.percentage !== null) parts.push(`${s.percentage.toFixed(0)}%`);
        if (s.state) parts.push(s.state);
        if (s.time_to_empty_secs !== null)
            parts.push(`${Math.round(s.time_to_empty_secs / 60)} min to empty`);
        if (s.time_to_full_secs !== null)
            parts.push(`${Math.round(s.time_to_full_secs / 60)} min to full`);
        return parts.join(' \u00b7 ');
    }
</script>

{#if state.available}
    <div class="tray-icon battery" title={tooltipFor(state)}>
        <span class="icon" aria-hidden="true"
            >{batteryIcon(state.percentage, state.state === 'charging')}</span
        >
        <Ring percent={state.percentage} color={gradientColor(state.percentage)} />
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
        cursor: default;
        line-height: 1;
    }
    .icon {
        /*
         * Nerd Font glyph. JetBrains Mono Nerd Font (and most other
         * Nerd Font variants) provide the icons in the private-use
         * area; we list it first so the icon renders properly while
         * the rest of the bar still uses the sans-serif body font.
         */
        font-family:
            'JetBrainsMono Nerd Font',
            'Symbols Nerd Font',
            'FontAwesome',
            var(--font-mono, ui-monospace, monospace);
        font-size: var(--tray-icon-size, 14px);
        line-height: 1;
    }
</style>
