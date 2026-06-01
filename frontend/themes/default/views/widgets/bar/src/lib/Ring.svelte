<script lang="ts">
    /**
     * Compact SVG donut ring used across the bar. Two concentric
     * circles (track + active arc); the arc's stroke length is driven
     * by stroke-dasharray + stroke-dashoffset so it fills proportionally
     * to `percent`. Stroke color is supplied by the caller so each
     * indicator can map the value to its own gradient.
     *
     * The center hosts an optional `label` — either a number (CPU%,
     * MEM%) or an icon glyph (battery, sun, wifi, etc). The `kind`
     * prop selects the font family so number labels use the monospace
     * tabular digits and icon labels use the Nerd Font.
     *
     * `percent: null` renders an empty ring (just the track) and the
     * label still draws. Used pre-first-sample so the indicator's
     * width stays stable while data loads.
     */
    interface Props {
        percent: number | null;
        /** Outer diameter in px. Default 18. */
        size?: number;
        /** Stroke width. Default 2. */
        stroke?: number;
        /** Optional inline text rendered at the ring's center. */
        label?: string;
        /**
         * How to render the label. `'number'` (default) uses a small
         * monospace digit; `'icon'` uses the Nerd Font stack so glyphs
         * from the private-use area render correctly. The label color
         * inherits from CSS in both cases.
         */
        kind?: 'number' | 'icon';
        /** CSS color used for the active arc; defaults to currentColor. */
        color?: string;
        /**
         * Override the auto-derived label font-size. Useful when an icon
         * needs to be larger (or smaller) than the default 60% of the
         * ring diameter.
         */
        labelSize?: number;
    }

    let {
        percent,
        size = 20,
        stroke = 2,
        label,
        kind = 'number',
        color = 'currentColor',
        labelSize,
    }: Props = $props();

    const radius = $derived((size - stroke) / 2);
    const circ = $derived(2 * Math.PI * radius);
    const dashoffset = $derived(
        percent === null ? circ : circ * (1 - Math.max(0, Math.min(100, percent)) / 100),
    );

    // Default label size scales with the ring. Numeric labels sit at
    // ~42% of the ring diameter (room for 2-3 digits). Icons sit at
    // ~70% so the glyph fills the ring properly — Nerd Font icons
    // ship inside a fairly small em-square, so a higher coefficient
    // gets them visually centered without crowding the stroke.
    const resolvedLabelSize = $derived(
        labelSize ?? (kind === 'icon' ? Math.round(size * 0.7) : Math.round(size * 0.42)),
    );

    const TRACK_COLOR = 'rgba(255, 255, 255, 0.12)';
</script>

<svg
    class="ring"
    class:ring-icon={kind === 'icon'}
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
            font-size={resolvedLabelSize}
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
        font-weight: 600;
        fill: var(--color-fg, #cdd6f4);
        font-variant-numeric: tabular-nums;
        pointer-events: none;
    }
    /*
     * Icon-mode label uses the Nerd Font stack so private-use-area
     * glyphs render properly. Slightly lighter weight because Nerd
     * Font glyphs already have their own visual weight baked in.
     */
    .ring-icon .ring-label {
        font-family:
            'JetBrainsMono Nerd Font',
            'Symbols Nerd Font',
            'FontAwesome',
            var(--font-mono, ui-monospace, monospace);
        font-weight: 400;
    }
</style>
