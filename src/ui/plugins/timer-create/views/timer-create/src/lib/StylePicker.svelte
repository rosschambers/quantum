<script lang="ts">
    import type { VisualStyle } from '@quantum/client';

    /**
     * Style picker for a new timer. Renders the six supported styles as small
     * live example swatches (no text labels) and emits the chosen
     * `VisualStyle` through `onChange`. The selected swatch is highlighted.
     */
    let {
        style = 'ring',
        accentHue = 210,
        onChange,
    }: {
        style?: VisualStyle;
        accentHue?: number;
        onChange: (next: VisualStyle) => void;
    } = $props();

    /** Only the six distinct styles — no `mixed`, no `wedge`. */
    const STYLES: VisualStyle[] = ['ring', 'pie', 'dots', 'bar', 'spiral', 'pulse'];

    /** Swatch box size in pixels; the example renders inside at ~60%. */
    const SWATCH_SIZE = 70;
    const VISUAL_SIZE = Math.round(SWATCH_SIZE * 0.6);
    /** A fixed fraction so each swatch shows a partially-filled example. */
    const FRACTION = 0.66;

    const stroke = $derived(`hsl(${accentHue} 70% 62%)`);
    const track = $derived(`hsl(${accentHue} 30% 50% / 0.25)`);

    // Ring geometry.
    const ringStrokeWidth = 5;
    const ringCenter = VISUAL_SIZE / 2;
    const ringRadius = (VISUAL_SIZE - ringStrokeWidth) / 2;
    const ringCircumference = $derived(2 * Math.PI * ringRadius);
    const ringDashOffset = $derived(ringCircumference * (1 - FRACTION));

    // Pie geometry.
    const pieAngle = FRACTION * 360;

    // Dot grid geometry.
    const DOT_COUNT = 9;
    const dotsLit = Math.round(FRACTION * DOT_COUNT);
    const dots = Array.from({ length: DOT_COUNT }, (_, index) => index < dotsLit);
    const dotSize = Math.round(VISUAL_SIZE * 0.17);
    const dotGap = Math.round(VISUAL_SIZE * 0.06);

    // Bar geometry.
    const barWidth = Math.round(VISUAL_SIZE * 0.82);
    const barHeight = 11;

    /**
     * Build an Archimedean spiral path string growing from the centre outward,
     * starting at twelve o'clock. `fraction` truncates the spiral so a partial
     * example matches the other swatches' partially-filled look.
     */
    function buildSpiralPath(
        centerX: number,
        centerY: number,
        maxRadius: number,
        turns: number,
        fraction: number,
    ): string {
        const steps = 120;
        const maxTheta = turns * 2 * Math.PI * fraction;
        let definition = '';
        for (let index = 0; index <= steps; index += 1) {
            const theta = (index / steps) * maxTheta;
            const radius = (theta / (turns * 2 * Math.PI)) * maxRadius;
            const x = centerX + radius * Math.cos(theta - Math.PI / 2);
            const y = centerY + radius * Math.sin(theta - Math.PI / 2);
            definition += (index === 0 ? 'M' : 'L') + x.toFixed(1) + ' ' + y.toFixed(1) + ' ';
        }
        return definition;
    }

    // Spiral geometry. Reuses the ring stroke width for a consistent weight.
    const spiralTurns = 3;
    const spiralStrokeWidth = ringStrokeWidth;
    const spiralRadius = (VISUAL_SIZE - spiralStrokeWidth) / 2;
    const spiralTrackPath = buildSpiralPath(ringCenter, ringCenter, spiralRadius, spiralTurns, 1);
    const spiralFillPath = buildSpiralPath(
        ringCenter,
        ringCenter,
        spiralRadius,
        spiralTurns,
        FRACTION,
    );

    // Pulse geometry. A filled inner circle inside a faint outer ring.
    const pulseOuterRadius = (VISUAL_SIZE - ringStrokeWidth) / 2;
    const pulseInnerRadius = Math.round(VISUAL_SIZE * 0.32);

    function selectStyle(next: VisualStyle): void {
        style = next;
        onChange(next);
    }
</script>

<div class="style-picker" role="group" aria-label="Visual style">
    {#each STYLES as candidate (candidate)}
        <button
            type="button"
            class="swatch"
            class:active={style === candidate}
            data-style={candidate}
            aria-label={candidate}
            aria-pressed={style === candidate}
            onclick={() => selectStyle(candidate)}
        >
            {#if candidate === 'ring'}
                <svg
                    width={VISUAL_SIZE}
                    height={VISUAL_SIZE}
                    viewBox="0 0 {VISUAL_SIZE} {VISUAL_SIZE}"
                >
                    <circle
                        cx={ringCenter}
                        cy={ringCenter}
                        r={ringRadius}
                        fill="none"
                        stroke={track}
                        stroke-width={ringStrokeWidth}
                    />
                    <circle
                        cx={ringCenter}
                        cy={ringCenter}
                        r={ringRadius}
                        fill="none"
                        stroke={stroke}
                        stroke-width={ringStrokeWidth}
                        stroke-linecap="round"
                        stroke-dasharray={ringCircumference}
                        stroke-dashoffset={ringDashOffset}
                        transform="rotate(-90 {ringCenter} {ringCenter})"
                    />
                </svg>
            {:else if candidate === 'pie'}
                <span
                    class="pie"
                    style="width:{VISUAL_SIZE}px;height:{VISUAL_SIZE}px;background:conic-gradient({stroke} {pieAngle}deg, {track} {pieAngle}deg);"
                ></span>
            {:else if candidate === 'dots'}
                <span
                    class="dots"
                    style="width:{barWidth}px;gap:{dotGap}px;"
                >
                    {#each dots as lit}
                        <span
                            class="dot"
                            style="width:{dotSize}px;height:{dotSize}px;background:{lit
                                ? stroke
                                : track};"
                        ></span>
                    {/each}
                </span>
            {:else if candidate === 'bar'}
                <span
                    class="bar"
                    style="width:{barWidth}px;height:{barHeight}px;border-radius:{barHeight}px;background:{track};"
                >
                    <span
                        class="bar-fill"
                        style="width:{FRACTION * 100}%;background:{stroke};border-radius:{barHeight}px;"
                    ></span>
                </span>
            {:else if candidate === 'spiral'}
                <svg
                    width={VISUAL_SIZE}
                    height={VISUAL_SIZE}
                    viewBox="0 0 {VISUAL_SIZE} {VISUAL_SIZE}"
                >
                    <path
                        d={spiralTrackPath}
                        fill="none"
                        stroke={track}
                        stroke-width={spiralStrokeWidth}
                        stroke-linecap="round"
                    />
                    <path
                        d={spiralFillPath}
                        fill="none"
                        stroke={stroke}
                        stroke-width={spiralStrokeWidth}
                        stroke-linecap="round"
                    />
                </svg>
            {:else if candidate === 'pulse'}
                <svg
                    width={VISUAL_SIZE}
                    height={VISUAL_SIZE}
                    viewBox="0 0 {VISUAL_SIZE} {VISUAL_SIZE}"
                >
                    <circle
                        cx={ringCenter}
                        cy={ringCenter}
                        r={pulseOuterRadius}
                        fill="none"
                        stroke={track}
                        stroke-width={2}
                    />
                    <circle
                        cx={ringCenter}
                        cy={ringCenter}
                        r={pulseInnerRadius}
                        fill={stroke}
                    />
                </svg>
            {/if}
        </button>
    {/each}
</div>

<style>
    .style-picker {
        display: flex;
        flex-wrap: wrap;
        gap: 12px;
    }
    .swatch {
        width: 70px;
        height: 70px;
        display: flex;
        align-items: center;
        justify-content: center;
        border: 2px solid var(--color-border, #2a3142);
        border-radius: 14px;
        background: var(--color-bg, #1c2230);
        cursor: pointer;
        padding: 0;
        transition: border-color 0.12s, background 0.12s;
    }
    .swatch:hover {
        border-color: var(--color-accent, #5b9dff);
    }
    .swatch.active {
        border-color: var(--color-accent, #5b9dff);
        background: var(--color-accent-soft, rgba(91, 157, 255, 0.12));
    }
    .pie {
        border-radius: 50%;
        display: inline-block;
    }
    .dots {
        display: flex;
        flex-wrap: wrap;
        justify-content: center;
    }
    .dot {
        border-radius: 50%;
        display: inline-block;
    }
    .bar {
        display: inline-block;
        overflow: hidden;
    }
    .bar-fill {
        display: block;
        height: 100%;
    }
</style>
