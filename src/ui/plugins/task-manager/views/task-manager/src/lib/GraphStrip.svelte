<script lang="ts">
    import type { GlobalStats } from '@quantum/client';
    import { pushSample, smoothPath, hotColdColor } from './graphs';

    /**
     * Three side-by-side sparklines (CPU, memory, network) driven by the global
     * stats of each process snapshot. The component owns its rolling history: it
     * starts empty at mount and pushes one sample per snapshot, so the graphs
     * fill in from the moment the view opens (no backfill). CPU and memory scale
     * to a fixed 0-100; the network graph scales to its own rolling maximum.
     */
    interface Props {
        global: GlobalStats | null;
    }

    let { global }: Props = $props();

    // Viewbox geometry. Width is nominal — the SVG stretches to its container
    // via `preserveAspectRatio="none"` and `width: 100%`; height is the real
    // 56px strip height from the tuned design.
    const WIDTH = 320;
    const HEIGHT = 56;
    const BYTES_PER_GB = 1024 * 1024 * 1024;
    const BYTES_PER_MB = 1024 * 1024;

    // The rolling history lives in plain (non-reactive) variables. The push
    // effect reads and writes these freely, and its only tracked dependency is
    // `global` (a fresh object each snapshot). The `$state` mirrors below are
    // written but never read inside the effect, which is what keeps Svelte's
    // read-and-write guard from spinning the effect into an update loop.
    let historyCpu: number[] = [];
    let historyMem: number[] = [];
    let historyRx: number[] = [];
    let historyTx: number[] = [];

    let cpuSeries = $state<number[]>([]);
    let memSeries = $state<number[]>([]);
    let rxSeries = $state<number[]>([]);
    let txSeries = $state<number[]>([]);

    $effect(() => {
        const stats = global;
        if (stats === null) return;
        historyCpu = pushSample(historyCpu, clampPercent(stats.cpu_percent));
        historyMem = pushSample(historyMem, memoryPercent(stats));
        historyRx = pushSample(historyRx, toMegabytesPerSecond(stats.net_rx_bytes_per_second));
        historyTx = pushSample(historyTx, toMegabytesPerSecond(stats.net_tx_bytes_per_second));
        cpuSeries = historyCpu;
        memSeries = historyMem;
        rxSeries = historyRx;
        txSeries = historyTx;
    });

    function clampPercent(value: number): number {
        if (!Number.isFinite(value) || value < 0) return 0;
        if (value > 100) return 100;
        return value;
    }

    function memoryPercent(stats: GlobalStats): number {
        if (stats.mem_total_bytes === 0) return 0;
        return clampPercent((stats.mem_used_bytes / stats.mem_total_bytes) * 100);
    }

    function toMegabytesPerSecond(bytesPerSecond: number): number {
        if (!Number.isFinite(bytesPerSecond) || bytesPerSecond < 0) return 0;
        return bytesPerSecond / BYTES_PER_MB;
    }

    function last(series: number[]): number {
        return series.length === 0 ? 0 : series[series.length - 1];
    }

    /** Close a line path into a filled area by dropping to the baseline. */
    function fillPath(linePath: string): string {
        if (linePath === '') return '';
        return `${linePath} L${WIDTH},${HEIGHT} L0,${HEIGHT} Z`;
    }

    const currentCpu = $derived(last(cpuSeries));
    const currentMemPercent = $derived(last(memSeries));
    const currentRx = $derived(last(rxSeries));
    const currentTx = $derived(last(txSeries));

    // Network scales to its rolling max across both directions, never a fixed
    // 100; a floor of 1 keeps an all-idle series from dividing by zero.
    const netMax = $derived(Math.max(1, ...rxSeries, ...txSeries));

    const cpuPath = $derived(smoothPath(cpuSeries, WIDTH, HEIGHT, 100));
    const memPath = $derived(smoothPath(memSeries, WIDTH, HEIGHT, 100));
    const rxPath = $derived(smoothPath(rxSeries, WIDTH, HEIGHT, netMax));
    const txPath = $derived(smoothPath(txSeries, WIDTH, HEIGHT, netMax));

    // Strokes use the cool-to-hot gradient keyed to the current value, matching
    // the bar meters. The network stroke is keyed to rx as a fraction of its
    // own scale so a busy link still reads hot.
    const cpuColor = $derived(hotColdColor(currentCpu));
    const memColor = $derived(hotColdColor(currentMemPercent));
    const netColor = $derived(hotColdColor((currentRx / netMax) * 100));

    const usedGb = $derived(global === null ? 0 : global.mem_used_bytes / BYTES_PER_GB);
    const totalGb = $derived(global === null ? 0 : global.mem_total_bytes / BYTES_PER_GB);

    const cpuLabel = $derived(`${Math.round(currentCpu)} %`);
    const memLabel = $derived(`${usedGb.toFixed(1)} / ${totalGb.toFixed(1)} GB`);
    const netLabel = $derived(`down ${currentRx.toFixed(1)} up ${currentTx.toFixed(1)} MB/s`);
</script>

<div class="graphs">
    <div class="graph">
        <header>
            <span class="glabel">CPU</span>
            <span class="gval">{cpuLabel}</span>
        </header>
        <svg viewBox="0 0 {WIDTH} {HEIGHT}" height={HEIGHT} preserveAspectRatio="none" aria-hidden="true">
            {#if cpuPath}
                <path d={fillPath(cpuPath)} fill={cpuColor} opacity="0.13" />
                <path d={cpuPath} fill="none" stroke={cpuColor} stroke-width="1.6" stroke-linecap="round" />
            {/if}
        </svg>
    </div>

    <div class="graph">
        <header>
            <span class="glabel">Memory</span>
            <span class="gval">{memLabel}</span>
        </header>
        <svg viewBox="0 0 {WIDTH} {HEIGHT}" height={HEIGHT} preserveAspectRatio="none" aria-hidden="true">
            {#if memPath}
                <path d={fillPath(memPath)} fill={memColor} opacity="0.13" />
                <path d={memPath} fill="none" stroke={memColor} stroke-width="1.6" stroke-linecap="round" />
            {/if}
        </svg>
    </div>

    <div class="graph">
        <header>
            <span class="glabel">Network</span>
            <span class="gval">{netLabel}</span>
        </header>
        <svg viewBox="0 0 {WIDTH} {HEIGHT}" height={HEIGHT} preserveAspectRatio="none" aria-hidden="true">
            {#if rxPath}
                <path d={fillPath(rxPath)} fill={netColor} opacity="0.13" />
                <path d={rxPath} fill="none" stroke={netColor} stroke-width="1.6" stroke-linecap="round" />
            {/if}
            {#if txPath}
                <path
                    d={txPath}
                    fill="none"
                    stroke="var(--color-warning, #c96342)"
                    stroke-width="1.4"
                    stroke-linecap="round"
                />
            {/if}
        </svg>
    </div>
</div>

<style>
    .graphs {
        display: flex;
        gap: 10px;
        padding: 12px 14px 8px;
        flex: none;
    }
    .graph {
        flex: 1;
        min-width: 0;
        background: var(--color-bg-alt, #3b3630);
        border: 1px solid var(--color-border, #4a443e);
        border-radius: 10px;
        padding: 8px 10px 6px;
    }
    .graph header {
        display: flex;
        justify-content: space-between;
        align-items: baseline;
        margin-bottom: 4px;
    }
    .glabel {
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted, #8a8578);
    }
    .gval {
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 12px;
        font-variant-numeric: tabular-nums;
        color: var(--color-fg, #e8e4d8);
    }
    .graph svg {
        display: block;
        width: 100%;
    }
</style>
