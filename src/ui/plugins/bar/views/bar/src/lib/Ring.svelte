<script lang="ts">
    import Icon, { type IconName } from './Icon.svelte';

    /**
     * Compact donut ring with optional centered label. An SVG draws
     * the track + filled arc; an optional centered label sits in the
     * middle.
     *
     * Two label modes:
     * - Numeric (kind='number'): renders inside the SVG as <text>.
     *   Tabular digits center reliably this way.
     * - Icon (kind='icon' + iconName): renders an SVG icon overlay
     *   on top of the ring. Each icon component uses a uniform 24x24
     *   viewBox with content drawn centered, so visual centering
     *   "just works" without per-glyph offsets.
     *
     * `percent: null` renders an empty ring (just the track). The
     * label still draws so the indicator's width stays stable while
     * data loads.
     */
    interface Props {
        percent: number | null;
        /** Outer diameter in px. Default 22. */
        size?: number;
        /** Stroke width. Default 2.5 to visually match the inline-SVG icon
         * stroke weight (those use stroke-width=2 on a 24x24 viewBox,
         * roughly equivalent to ~2.5px on the 22px ring at the same
         * scale). */
        stroke?: number;
        /** Numeric/text label for `kind="number"` mode. */
        label?: string;
        /** Icon name for `kind="icon"` mode. */
        iconName?: IconName;
        /** Which mode this ring is in. */
        kind?: 'number' | 'icon';
        /** CSS color used for the active arc; defaults to currentColor. */
        color?: string;
        /** Override the auto-derived label font-size / icon size. */
        labelSize?: number;
    }

    let {
        percent,
        size = 22,
        stroke = 2.5,
        label,
        iconName,
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
     * ~60% so the SVG fills the ring with breathing room around it.
     */
    const resolvedLabelSize = $derived(
        labelSize ?? (kind === 'icon' ? Math.round(size * 0.6) : Math.round(size * 0.42)),
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
    {#if iconName && kind === 'icon'}
        <span class="icon-overlay">
            <Icon name={iconName} size={resolvedLabelSize} />
        </span>
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
        /* Short enough that the WebKit compositor returns to idle quickly
         * between 1Hz updates; long enough that the change still reads
         * as smooth instead of an abrupt jump. */
        transition: stroke-dashoffset 0.15s ease, stroke 0.15s ease;
    }
    .number-label {
        font-family: var(--font-mono, ui-monospace, monospace);
        font-weight: 600;
        fill: var(--color-fg, #cdd6f4);
        font-variant-numeric: tabular-nums;
        pointer-events: none;
    }
    /*
     * Icon overlay sits absolutely centered over the ring. The Icon
     * component itself uses a uniform 24x24 viewBox with content
     * drawn centered, so absolute-50%/translate(-50%) lands the
     * glyph at the ring's geometric center.
     */
    .icon-overlay {
        position: absolute;
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
        color: var(--color-fg, #cdd6f4);
        line-height: 0;
        pointer-events: none;
    }
</style>
