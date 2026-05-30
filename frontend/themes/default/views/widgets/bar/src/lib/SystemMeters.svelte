<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { SystemStats } from '../lib/types';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let stats: SystemStats | null = $state(null);

    /**
     * Rolling history of (cpu_percent, mem_percent) samples used to draw
     * the inline sparkline. Capped at HISTORY_LENGTH samples; new entries
     * push, old entries shift. Stats stream at 1 Hz so 32 samples covers
     * the last ~32 seconds.
     */
    const HISTORY_LENGTH = 32;
    let cpuHistory: number[] = $state([]);
    let memHistory: number[] = $state([]);

    $effect(() => {
        client
            .call('provider.query', { id: 'system.stats' })
            .then((r: unknown) => {
                if (r) pushStats(r as SystemStats);
            })
            .catch(() => {});
        const unsubscribe = client.subscribe('system.stats.event', (payload: unknown) => {
            pushStats(payload as SystemStats);
        });
        return () => unsubscribe?.();
    });

    function pushStats(s: SystemStats): void {
        stats = s;
        cpuHistory = [...cpuHistory, clampPercent(s.cpu_percent)].slice(-HISTORY_LENGTH);
        const memPct =
            s.mem_total_bytes === 0
                ? 0
                : clampPercent((s.mem_used_bytes / s.mem_total_bytes) * 100);
        memHistory = [...memHistory, memPct].slice(-HISTORY_LENGTH);
    }

    function clampPercent(v: number): number {
        if (!Number.isFinite(v) || v < 0) return 0;
        if (v > 100) return 100;
        return v;
    }

    function cpuPercent(s: SystemStats | null): number | null {
        if (!s || !Number.isFinite(s.cpu_percent)) return null;
        return clampPercent(s.cpu_percent);
    }

    function memPercent(s: SystemStats | null): number | null {
        if (!s || s.mem_total_bytes === 0) return null;
        return clampPercent((s.mem_used_bytes / s.mem_total_bytes) * 100);
    }

    const CPU_ICON = '\u2699'; // gear
    const MEM_ICON = '\u25a3'; // white square containing black small square

    function bucketBar(pct: number | null): string {
        if (pct === null) return '   '; // three spaces: same width as bar glyphs
        if (pct < 33) return '\u25ae\u25af\u25af';
        if (pct < 67) return '\u25ae\u25ae\u25af';
        return '\u25ae\u25ae\u25ae';
    }

    function tooltipFor(label: string, pct: number | null): string {
        if (pct === null) return `${label}: unknown`;
        return `${label}: ${pct.toFixed(0)}%`;
    }

    /**
     * Build an SVG polyline path for the sparkline. Returns a coordinate
     * string like "0,12 4,8 8,5" for the `points` attribute. Empty
     * string when no history has accumulated yet.
     */
    function sparklinePoints(
        history: number[],
        width: number,
        height: number,
    ): string {
        if (history.length === 0) return '';
        const n = history.length;
        const stepX = n > 1 ? width / (n - 1) : 0;
        const points: string[] = [];
        for (let i = 0; i < n; i += 1) {
            const x = stepX * i;
            // Y is inverted (0 at top in SVG) and inset by 1px so the
            // line never lies flush against the top/bottom edges.
            const y = height - 1 - (history[i] / 100) * (height - 2);
            points.push(`${x.toFixed(2)},${y.toFixed(2)}`);
        }
        return points.join(' ');
    }

    const SPARKLINE_W = 36;
    const SPARKLINE_H = 14;
</script>

<div class="meters">
    <div class="meter cpu" title={tooltipFor('CPU', cpuPercent(stats))}>
        <span class="icon" aria-hidden="true">{CPU_ICON}</span>
        <span class="bar">{bucketBar(cpuPercent(stats))}</span>
        <svg
            class="sparkline"
            width={SPARKLINE_W}
            height={SPARKLINE_H}
            viewBox="0 0 {SPARKLINE_W} {SPARKLINE_H}"
            preserveAspectRatio="none"
            aria-hidden="true"
        >
            {#if cpuHistory.length > 1}
                <polyline
                    points={sparklinePoints(cpuHistory, SPARKLINE_W, SPARKLINE_H)}
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                />
            {/if}
        </svg>
    </div>
    <div class="meter mem" title={tooltipFor('MEM', memPercent(stats))}>
        <span class="icon" aria-hidden="true">{MEM_ICON}</span>
        <span class="bar">{bucketBar(memPercent(stats))}</span>
        <svg
            class="sparkline"
            width={SPARKLINE_W}
            height={SPARKLINE_H}
            viewBox="0 0 {SPARKLINE_W} {SPARKLINE_H}"
            preserveAspectRatio="none"
            aria-hidden="true"
        >
            {#if memHistory.length > 1}
                <polyline
                    points={sparklinePoints(memHistory, SPARKLINE_W, SPARKLINE_H)}
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                />
            {/if}
        </svg>
    </div>
</div>

<style>
    .meters {
        display: flex;
        gap: var(--space-3, 0.75rem);
        font-size: var(--font-size-sm, 12px);
        color: var(--color-fg-alt, #a6adc8);
    }
    .meter {
        display: flex;
        align-items: center;
        gap: 4px;
        line-height: 1;
        cursor: default;
    }
    .icon {
        font-size: 12px;
        opacity: 0.9;
    }
    .bar {
        font-family: var(--font-mono, monospace);
        font-size: 10px;
        letter-spacing: 1px;
        line-height: 1;
    }
    .sparkline {
        display: block;
        color: var(--color-fg-alt, #a6adc8);
    }
</style>
