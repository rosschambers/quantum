<script lang="ts" module>
    export type IconName = 'close' | 'app' | 'process' | 'search';
</script>

<script lang="ts">
    /**
     * Hand-drawn SVG icon set for the task manager panel. Each glyph is
     * authored on a 24x24 viewBox and strokes with currentColor so callers
     * drive colour through CSS. Paths are ported verbatim from the design
     * playground's icon markup; per-icon stroke widths are preserved so glyphs
     * keep the weight they were tuned at. Later tasks extend this set as the
     * tree and kill menu need more glyphs. No emoji, per the house rule.
     */
    interface Props {
        name: IconName;
        /** Width and height in px. Default 24. */
        size?: number;
        /** Stroke colour override; otherwise inherits currentColor. */
        color?: string;
    }

    let { name, size = 24, color = 'currentColor' }: Props = $props();

    // Stroke widths matching the playground; anything not listed uses 2.
    const strokeFor: Partial<Record<IconName, number>> = {
        close: 2.2,
    };
    const stroke = $derived(strokeFor[name] ?? 2);
</script>

<svg
    class="icon"
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke={color}
    stroke-width={stroke}
    aria-hidden="true"
>
    {#if name === 'close'}
        <path d="M5 5l14 14M19 5L5 19" />
    {:else if name === 'app'}
        <rect x="3" y="4" width="18" height="14" rx="2" />
        <path d="M8 21h8M12 18v3" />
    {:else if name === 'process'}
        <rect x="6" y="6" width="12" height="12" rx="2" />
        <path d="M12 2v4M12 18v4M2 12h4M18 12h4" />
    {:else if name === 'search'}
        <circle cx="11" cy="11" r="7" />
        <path d="M21 21l-4.5-4.5" />
    {/if}
</svg>

<style>
    .icon {
        display: block;
        flex-shrink: 0;
    }
</style>
