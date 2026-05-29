<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { PowerState } from '../types';
    import { POWER_CHANNEL } from '../channels';

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
        client.call('provider.query', { id: 'power' })
            .then((r: unknown) => { if (r) state = r as PowerState; })
            .catch(() => {});
        const unsubscribe = client.subscribe(POWER_CHANNEL, (payload: unknown) => {
            state = payload as PowerState;
        });
        return () => unsubscribe?.();
    });

    function iconFor(s: PowerState): string {
        if (!s.available || s.percentage === null) return '';
        // Choose icon based on charging state + percentage bucket (10/20/.../100).
        const charging = s.state === 'charging';
        const bucket = Math.max(0, Math.min(100, Math.round(s.percentage / 10) * 10));
        // Use simple character glyphs that all renderers support.
        // Charging gets a lightning-bolt prefix.
        // 10/20/.../90 = quarter-block glyphs, 100 = full block.
        // For simplicity here we just return a string like "⚡85%" or "85%".
        return `${charging ? '⚡' : ''}${bucket}%`;
    }

    function tooltipFor(s: PowerState): string {
        if (!s.available) return 'battery unavailable';
        const parts: string[] = [];
        if (s.percentage !== null) parts.push(`${s.percentage.toFixed(0)}%`);
        if (s.state) parts.push(s.state);
        if (s.time_to_empty_secs !== null) parts.push(`${Math.round(s.time_to_empty_secs / 60)} min to empty`);
        if (s.time_to_full_secs !== null) parts.push(`${Math.round(s.time_to_full_secs / 60)} min to full`);
        return parts.join(' · ');
    }
</script>

{#if state.available}
    <div class="tray-icon battery" title={tooltipFor(state)}>{iconFor(state)}</div>
{/if}

<style>
    .tray-icon {
        font-size: var(--tray-icon-size, 14px);
        color: var(--tray-icon-color, var(--color-fg, #cdd6f4));
        padding: 0 4px;
        user-select: none;
        cursor: default;
    }
</style>
