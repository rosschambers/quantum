<script lang="ts">
    /**
     * Compact SVG donut ring used by the tray indicators and the
     * SystemMeters. Two circles (track + arc); the arc's stroke length
     * is driven by stroke-dasharray + stroke-dashoffset so it fills
     * proportionally to `percent`. Stroke color comes from the parent
     * style (currentColor) so the caller controls the gradient mapping.
     *
     * Pass `null` for percent to render an empty ring (just the track).
     * Used pre-first-sample so the indicator's slot stays the right
     * width while data loads.
     */
    interface Props {
        percent: number | null;
        /** Outer diameter in px. Default 18. */
        size?: number;
        /** Stroke width. Default 2. */
        stroke?: number;
        /** Optional inline text rendered at the ring's center. */
        label?: string;
        /** CSS color used for the active arc; defaults to currentColor. */
        color?: string;
    }

    let {
        percent,
        size = 18,
        stroke = 2,
        label,
        color = 'currentColor',
    }: Props = $props();

    const radius = $derived((size - stroke) / 2);
    const circ = $derived(2 * Math.PI * radius);
    const dashoffset = $derived(
        percent === null ? circ : circ * (1 - Math.max(0, Math.min(100, percent)) / 100),
    );

    const TRACK_COLOR = 'rgba(255, 255, 255, 0.12)';
</script>

<svg
    class="ring"
    width={size}
    height={size}
    viewBox="0 0 {size} {size}"
    role="img"
>
    <circle
        cx={size / 2}
        cy={size / 2}
        r={radius}
        fill="none"
        stroke={TRACK_COLOR}
        stroke-width={stroke}
    />
    <circle
        class="ring-fill"
        cx={size / 2}
        cy={size / 2}
        r={radius}
        fill="none"
        stroke={color}
        stroke-width={stroke}
        stroke-linecap="round"
        stroke-dasharray={circ}
        stroke-dashoffset={dashoffset}
        transform="rotate(-90 {size / 2} {size / 2})"
    />
    {#if label}
        <text
            class="ring-label"
            x={size / 2}
            y={size / 2}
            text-anchor="middle"
            dominant-baseline="central"
        >{label}</text>
    {/if}
</svg>

<style>
    .ring { display: block; }
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
</style>
