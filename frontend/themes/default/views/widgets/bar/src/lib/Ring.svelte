<script lang="ts">
    /**
     * Compact donut ring used across the bar. An SVG draws the track
     * + filled arc; an optional centered label sits in the middle.
     * Numeric labels (`kind="number"`) render inside the SVG as
     * `<text>` because tabular digits center reliably that way. Icon
     * labels (`kind="icon"`) render as an HTML `<span>` overlaid by
     * CSS flexbox over the SVG — SVG text positioning of Nerd Font
     * glyphs is unreliable because each glyph occupies a different
     * spot within the font's em-square, but HTML's optical centering
     * with `line-height: 1` + flex centers them visually correctly.
     *
     * `percent: null` renders an empty ring (just the track). The
     * label still draws so the indicator's width stays stable while
     * data loads.
     */
    interface Props {
        percent: number | null;
        /** Outer diameter in px. Default 22. */
        size?: number;
        /** Stroke width. Default 2. */
        stroke?: number;
        /** Optional inline text rendered at the ring's center. */
        label?: string;
        /**
         * How to render the label. `'number'` (default) uses a small
         * monospace digit centered as SVG text. `'icon'` uses an HTML
         * span overlay with the Nerd Font stack so private-use-area
         * glyphs render correctly and visually center via flexbox.
         */
        kind?: 'number' | 'icon';
        /** CSS color used for the active arc; defaults to currentColor. */
        color?: string;
        /** Override the auto-derived label font-size. */
        labelSize?: number;
    }

    let {
        percent,
        size = 22,
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

    /**
     * Default label size scales with the ring. Numeric labels sit at
     * ~42% of the ring diameter (room for 2-3 digits). Icons sit at
     * ~58% so the glyph reads clearly while still leaving breathing
     * room around the stroke.
     */
    const resolvedLabelSize = $derived(
        labelSize ?? (kind === 'icon' ? Math.round(size * 0.58) : Math.round(size * 0.42)),
    );

    const TRACK_COLOR = 'rgba(255, 255, 255, 0.12)';
</script>

<span class="ring-wrap" style="width: {size}px; height: {size}px;">
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
        {#if label && kind === 'number'}
            <text
                class="ring-label number-label"
                x={size / 2}
                y={size / 2}
                text-anchor="middle"
                dominant-baseline="central"
                font-size={resolvedLabelSize}
            >{label}</text>
        {/if}
    </svg>
    {#if label && kind === 'icon'}
        <span
            class="icon-label"
            style="font-size: {resolvedLabelSize}px;"
            aria-hidden="true"
        >{label}</span>
    {/if}
</span>

<style>
    .ring-wrap {
        display: inline-flex;
        position: relative;
        align-items: center;
        justify-content: center;
        line-height: 0;
    }
    .ring { display: block; }
    .ring-fill {
        transition: stroke-dashoffset 0.4s ease, stroke 0.4s ease;
    }
    .number-label {
        font-family: var(--font-mono, ui-monospace, monospace);
        font-weight: 600;
        fill: var(--color-fg, #cdd6f4);
        font-variant-numeric: tabular-nums;
        pointer-events: none;
    }
    /*
     * Icon label overlays the SVG. Flexbox on the wrapper aligns it
     * to the ring's geometric center; line-height: 1 strips off any
     * extra leading so Nerd Font glyphs sit on the visual middle
     * line instead of being pushed down by font metrics.
     */
    .icon-label {
        position: absolute;
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
        font-family:
            'JetBrainsMono Nerd Font',
            'Symbols Nerd Font',
            'FontAwesome',
            var(--font-mono, ui-monospace, monospace);
        color: var(--color-fg, #cdd6f4);
        line-height: 1;
        font-weight: 400;
        pointer-events: none;
        /* Optical compensation for Nerd Font glyphs' asymmetric
         * vertical metrics (most have more ink in the upper half).
         * Without this nudge they hang noticeably low. */
        margin-top: -0.05em;
    }
</style>
