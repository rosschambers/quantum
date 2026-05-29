<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { SystemStats } from '../lib/types';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let stats: SystemStats | null = $state(null);

    // `$effect` runs reliably in testing-library + Svelte 5 where `onMount` does not.
    // Behaviour at runtime in WebKit is the same: subscribe on mount, unsubscribe on unmount.
    $effect(() => {
        const unsubscribe = client.subscribe('system.stats.event', (payload: unknown) => {
            stats = payload as SystemStats;
        });
        return () => unsubscribe?.();
    });

    function cpuLabel(s: SystemStats | null): string {
        if (!s) return 'CPU --%';
        return `CPU ${Math.round(s.cpu_percent)}%`;
    }

    function memLabel(s: SystemStats | null): string {
        if (!s) return 'MEM --%';
        if (s.mem_total_bytes === 0) return 'MEM --%';
        const pct = Math.round((s.mem_used_bytes / s.mem_total_bytes) * 100);
        return `MEM ${pct}%`;
    }
</script>

<div class="meters">
    <span class="meter cpu">{cpuLabel(stats)}</span>
    <span class="meter mem">{memLabel(stats)}</span>
</div>

<style>
    .meters {
        display: flex;
        gap: var(--space-3, 0.75rem);
        font-variant-numeric: tabular-nums;
        font-size: var(--font-size-sm, 12px);
        color: var(--color-fg-alt, #a6adc8);
    }
</style>
