<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { SystemStats } from '../lib/types';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let stats: SystemStats | null = $state(null);

    /**
     * Rolling history of CPU% and MEM% samples used to draw the inline
     * sparkline. Stats arrive at roughly 1 Hz; HISTORY_LENGTH samples
     * therefore covers about half a minute of history. Each new sample
     * pushes; old samples drop off the left.
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
        const cpu = clampPercent(s.cpu_percent);
        const mem = s.mem_total_bytes === 0
            ? 0
            : clampPercent((s.mem_used_bytes / s.mem_total_bytes) * 100);
        cpuHistory = [...cpuHistory, cpu].slice(-HISTORY_LENGTH);
        memHistory = [...memHistory, mem].slice(-HISTORY_LENGTH);
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

    function tooltipFor(label: string, pct: number | null): string {
        if (pct === null) return `${label}: unknown`;
        return `${label}: ${pct.toFixed(0)}%`;
    }

    /**
     * Smooth cool-to-hot gradient. 0% maps to a cool blue, 100% maps to
     * a hot red, with a yellow-ish midpoint coming naturally from
     * linear interpolation across all three channels. Shared between
     * the active ring stroke and the sparkline stroke so the two read
     * as the same metric.
     */
    function gradientColor(pct: number | null): string {
        const p = pct === null ? 0 : pct;
        const r = Math.round(80 + (p / 100) * 175);
        const g = Math.round(180 - (p / 100) * 130);
        const b = Math.round(250 - (p / 100) * 200);
        return `rgb(${r}, ${g}, ${b})`;
    }

    // Ring geometry.
    const RING_SIZE = 22;
    const STROKE = 2;
    const RADIUS = (RING_SIZE - STROKE) / 2;
    const CIRC = 2 * Math.PI * RADIUS;

    function dashOffsetFor(pct: number | null): number {
        const p = pct ?? 0;
        return CIRC * (1 - p / 100);
    }

    function ringLabel(pct: number | null): string {
        if (pct === null) return '--';
        return `${Math.round(pct)}`;
    }

    // Sparkline geometry. Sized to sit flush against the ring at
    // ~22px tall so the meter as a whole reads as a unit.
    const SPARK_W = 40;
    const SPARK_H = 18;
    const SPARK_PAD = 2; // top/bottom pad so the stroke doesn't clip

    /**
     * Build a smoothed SVG `d` attribute from a samples array using a
     * Catmull-Rom-to-Bezier conversion (tension 0.5, the standard
     * "uniform" Catmull-Rom). Two control points per segment make the
     * line continuously differentiable and visibly softer than the raw
     * polyline. Returns an empty string when there are fewer than two
     * samples (no curve to draw).
     *
     * Spline math:
     *   For each pair (p1, p2) we look at the surrounding points
     *   (p0, p1, p2, p3) and produce two cubic bezier control points:
     *     cp1 = p1 + (p2 - p0) / 6
     *     cp2 = p2 - (p3 - p1) / 6
     *   At the endpoints we duplicate p0=p1 or p3=p2 to avoid running
     *   off the array.
     */
    function smoothPath(samples: number[], width: number, height: number): string {
        if (samples.length < 2) return '';
        const n = samples.length;
        const padTop = SPARK_PAD;
        const padBot = SPARK_PAD;
        const drawableHeight = height - padTop - padBot;
        const stepX = n > 1 ? width / (n - 1) : 0;
        const points: Array<[number, number]> = samples.map((v, i) => {
            const x = i * stepX;
            const y = padTop + (1 - v / 100) * drawableHeight;
            return [x, y];
        });
        const segments: string[] = [];
        segments.push(`M${points[0][0].toFixed(2)},${points[0][1].toFixed(2)}`);
        for (let i = 0; i < points.length - 1; i += 1) {
            const p0 = points[i - 1] ?? points[i];
            const p1 = points[i];
            const p2 = points[i + 1];
            const p3 = points[i + 2] ?? p2;
            const cp1x = p1[0] + (p2[0] - p0[0]) / 6;
            const cp1y = p1[1] + (p2[1] - p0[1]) / 6;
            const cp2x = p2[0] - (p3[0] - p1[0]) / 6;
            const cp2y = p2[1] - (p3[1] - p1[1]) / 6;
            segments.push(
                `C${cp1x.toFixed(2)},${cp1y.toFixed(2)} ${cp2x.toFixed(2)},${cp2y.toFixed(2)} ${p2[0].toFixed(2)},${p2[1].toFixed(2)}`,
            );
        }
        return segments.join(' ');
    }

    const TRACK_COLOR = 'rgba(255, 255, 255, 0.12)';
</script>

<div class="meters">
    <div class="meter cpu" title={tooltipFor('CPU', cpuPercent(stats))}>
        <svg
            class="ring"
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
        <svg
            class="sparkline"
            width={SPARK_W}
            height={SPARK_H}
            viewBox="0 0 {SPARK_W} {SPARK_H}"
            preserveAspectRatio="none"
            aria-hidden="true"
        >
            {#if cpuHistory.length > 1}
                <path
                    d={smoothPath(cpuHistory, SPARK_W, SPARK_H)}
                    fill="none"
                    stroke={gradientColor(cpuPercent(stats))}
                    stroke-width="1.2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                />
            {/if}
        </svg>
    </div>
    <div class="meter mem" title={tooltipFor('MEM', memPercent(stats))}>
        <svg
            class="ring"
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
        <svg
            class="sparkline"
            width={SPARK_W}
            height={SPARK_H}
            viewBox="0 0 {SPARK_W} {SPARK_H}"
            preserveAspectRatio="none"
            aria-hidden="true"
        >
            {#if memHistory.length > 1}
                <path
                    d={smoothPath(memHistory, SPARK_W, SPARK_H)}
                    fill="none"
                    stroke={gradientColor(memPercent(stats))}
                    stroke-width="1.2"
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
        align-items: center;
    }
    .meter {
        display: inline-flex;
        align-items: center;
        gap: 2px;
        cursor: default;
        line-height: 0;
    }
    .meter svg.ring,
    .meter svg.sparkline {
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
        pointer-events: none;
    }
    .sparkline path {
        transition: stroke 0.4s ease;
    }
</style>
