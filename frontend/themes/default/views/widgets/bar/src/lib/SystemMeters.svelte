<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { SystemStats } from '../lib/types';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let stats: SystemStats | null = $state(null);

    $effect(() => {
        client
            .call('provider.query', { id: 'system.stats' })
            .then((r: unknown) => {
                if (r) stats = r as SystemStats;
            })
            .catch(() => {});
        const unsubscribe = client.subscribe('system.stats.event', (payload: unknown) => {
            stats = payload as SystemStats;
        });
        return () => unsubscribe?.();
    });

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

    function tooltipFor(label: string, pct: number | null): string {
        if (pct === null) return `${label}: unknown`;
        return `${label}: ${pct.toFixed(0)}%`;
    }

    /**
     * Smooth cool-to-hot gradient. 0% maps to a cool blue, 100% maps to
     * a hot red. Linearly interpolated through a medium-saturation
     * yellow-ish midpoint. Used both for the active ring stroke and any
     * downstream styling that wants to color by load.
     */
    function gradientColor(pct: number | null): string {
        const p = pct === null ? 0 : pct;
        const r = Math.round(80 + (p / 100) * 175);
        const g = Math.round(180 - (p / 100) * 130);
        const b = Math.round(250 - (p / 100) * 200);
        return `rgb(${r}, ${g}, ${b})`;
    }

    // Ring geometry. Diameter, stroke width, and computed radius +
    // circumference. The stroke is positioned with a half-pixel inset so
    // it doesn't get clipped by the SVG viewBox.
    const RING_SIZE = 22;
    const STROKE = 2;
    const RADIUS = (RING_SIZE - STROKE) / 2;
    const CIRC = 2 * Math.PI * RADIUS;

    function dashOffsetFor(pct: number | null): number {
        const p = pct ?? 0;
        return CIRC * (1 - p / 100);
    }

    function ringLabel(pct: number | null): string {
        // No percent sign: the ring is the percent indicator. `--` while
        // we wait for the first sample, otherwise a 0-100 integer.
        if (pct === null) return '--';
        return `${Math.round(pct)}`;
    }

    const TRACK_COLOR = 'rgba(255, 255, 255, 0.12)';
</script>

<div class="meters">
    <div class="meter cpu" title={tooltipFor('CPU', cpuPercent(stats))}>
        <svg
            width={RING_SIZE}
            height={RING_SIZE}
            viewBox="0 0 {RING_SIZE} {RING_SIZE}"
            aria-label={tooltipFor('CPU', cpuPercent(stats))}
            role="img"
        >
            <circle
                cx={RING_SIZE / 2}
                cy={RING_SIZE / 2}
                r={RADIUS}
                fill="none"
                stroke={TRACK_COLOR}
                stroke-width={STROKE}
            />
            <circle
                class="ring-fill"
                cx={RING_SIZE / 2}
                cy={RING_SIZE / 2}
                r={RADIUS}
                fill="none"
                stroke={gradientColor(cpuPercent(stats))}
                stroke-width={STROKE}
                stroke-linecap="round"
                stroke-dasharray={CIRC}
                stroke-dashoffset={dashOffsetFor(cpuPercent(stats))}
                transform="rotate(-90 {RING_SIZE / 2} {RING_SIZE / 2})"
            />
            <text
                class="ring-label"
                x={RING_SIZE / 2}
                y={RING_SIZE / 2}
                text-anchor="middle"
                dominant-baseline="central"
            >{ringLabel(cpuPercent(stats))}</text>
        </svg>
    </div>
    <div class="meter mem" title={tooltipFor('MEM', memPercent(stats))}>
        <svg
            width={RING_SIZE}
            height={RING_SIZE}
            viewBox="0 0 {RING_SIZE} {RING_SIZE}"
            aria-label={tooltipFor('MEM', memPercent(stats))}
            role="img"
        >
            <circle
                cx={RING_SIZE / 2}
                cy={RING_SIZE / 2}
                r={RADIUS}
                fill="none"
                stroke={TRACK_COLOR}
                stroke-width={STROKE}
            />
            <circle
                class="ring-fill"
                cx={RING_SIZE / 2}
                cy={RING_SIZE / 2}
                r={RADIUS}
                fill="none"
                stroke={gradientColor(memPercent(stats))}
                stroke-width={STROKE}
                stroke-linecap="round"
                stroke-dasharray={CIRC}
                stroke-dashoffset={dashOffsetFor(memPercent(stats))}
                transform="rotate(-90 {RING_SIZE / 2} {RING_SIZE / 2})"
            />
            <text
                class="ring-label"
                x={RING_SIZE / 2}
                y={RING_SIZE / 2}
                text-anchor="middle"
                dominant-baseline="central"
            >{ringLabel(memPercent(stats))}</text>
        </svg>
    </div>
</div>

<style>
    .meters {
        display: flex;
        gap: var(--space-3, 0.75rem);
        align-items: center;
    }
    .meter {
        display: inline-flex;
        align-items: center;
        cursor: default;
        line-height: 0;
    }
    .meter svg {
        display: block;
    }
    .ring-fill {
        transition: stroke-dashoffset 0.4s ease, stroke 0.4s ease;
    }
    .ring-label {
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 9px;
        font-weight: 600;
        fill: var(--color-fg, #cdd6f4);
        font-variant-numeric: tabular-nums;
        /* pointer-events off so the parent .meter title tooltip still
         * fires when hovering over the number itself */
        pointer-events: none;
    }
</style>
