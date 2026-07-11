<script lang="ts" module>
    export type IconName =
        | 'folder'
        | 'file'
        | 'image'
        | 'code'
        | 'music'
        | 'archive'
        | 'document'
        | 'drive'
        | 'home'
        | 'pin'
        | 'back'
        | 'forward'
        | 'up'
        | 'chevron'
        | 'search'
        | 'columns'
        | 'link'
        | 'dotfile'
        | 'grid'
        | 'list'
        | 'eye'
        | 'close';
</script>

<script lang="ts">
    /**
     * Hand-drawn SVG icon set for the files panel. Each glyph is authored on
     * a 24x24 viewBox and strokes with currentColor so callers drive colour
     * through CSS. Paths are ported verbatim from the design playground's
     * icon object; per-icon stroke widths are preserved so glyphs keep the
     * weight they were tuned at. No emoji, per the house rule.
     */
    interface Props {
        name: IconName;
        /** Width and height in px. Default 24. */
        size?: number;
        /** Stroke colour override; otherwise inherits currentColor. */
        color?: string;
    }

    let { name, size = 24, color = 'currentColor' }: Props = $props();

    // Stroke widths matching the playground; anything not listed uses 1.8.
    const heavierStroke: Partial<Record<IconName, number>> = {
        back: 2,
        forward: 2,
        up: 2,
        chevron: 3,
        search: 2,
    };
    const stroke = $derived(heavierStroke[name] ?? 1.8);
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
    {#if name === 'folder'}
        <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
    {:else if name === 'file'}
        <path d="M6 3h8l4 4v14H6z" />
        <path d="M14 3v4h4" />
    {:else if name === 'image'}
        <rect x="3" y="5" width="18" height="14" rx="2" />
        <circle cx="9" cy="10" r="1.6" />
        <path d="M4 17l5-4 3 2 4-4 4 4" />
    {:else if name === 'code'}
        <path d="M9 8l-4 4 4 4M15 8l4 4-4 4" />
    {:else if name === 'music'}
        <path d="M9 18V6l10-2v12" />
        <circle cx="6.5" cy="18" r="2.5" />
        <circle cx="16.5" cy="16" r="2.5" />
    {:else if name === 'archive'}
        <rect x="4" y="4" width="16" height="5" rx="1" />
        <path d="M5 9v10a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V9M10 13h4" />
    {:else if name === 'document'}
        <path d="M6 3h8l4 4v14H6z" />
        <path d="M14 3v4h4M9 12h6M9 15h6" />
    {:else if name === 'drive'}
        <rect x="3" y="9" width="18" height="7" rx="2" />
        <circle cx="17" cy="12.5" r="1" fill={color} />
    {:else if name === 'home'}
        <path d="M4 11l8-7 8 7v9a1 1 0 0 1-1 1h-5v-6h-4v6H5a1 1 0 0 1-1-1v-9z" />
    {:else if name === 'pin'}
        <path d="M12 3l3 3-1 6 4 4h-5l-1 5-1-5H6l4-4-1-6z" />
    {:else if name === 'back'}
        <path d="M15 5l-7 7 7 7" />
    {:else if name === 'forward'}
        <path d="M9 5l7 7-7 7" />
    {:else if name === 'up'}
        <path d="M12 19V5M5 12l7-7 7 7" />
    {:else if name === 'chevron'}
        <path d="M9 5l7 7-7 7" />
    {:else if name === 'search'}
        <circle cx="10.5" cy="10.5" r="6" />
        <path d="M15.5 15.5L21 21" />
    {:else if name === 'columns'}
        <rect x="3" y="4" width="8" height="16" rx="1" />
        <rect x="13" y="4" width="8" height="16" rx="1" />
    {:else if name === 'link'}
        <path
            d="M10 14a4 4 0 0 0 6 0l3-3a4 4 0 0 0-6-6l-1 1M14 10a4 4 0 0 0-6 0l-3 3a4 4 0 0 0 6 6l1-1"
        />
    {:else if name === 'dotfile'}
        <circle cx="12" cy="12" r="8" stroke-dasharray="3 3" />
    {:else if name === 'grid'}
        <rect x="4" y="4" width="7" height="7" rx="1" />
        <rect x="13" y="4" width="7" height="7" rx="1" />
        <rect x="4" y="13" width="7" height="7" rx="1" />
        <rect x="13" y="13" width="7" height="7" rx="1" />
    {:else if name === 'list'}
        <path d="M8 6h12M8 12h12M8 18h12" />
        <circle cx="4.5" cy="6" r="1" fill={color} />
        <circle cx="4.5" cy="12" r="1" fill={color} />
        <circle cx="4.5" cy="18" r="1" fill={color} />
    {:else if name === 'eye'}
        <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6z" />
        <circle cx="12" cy="12" r="2.5" />
    {:else if name === 'close'}
        <path d="M6 6l12 12M18 6L6 18" />
    {/if}
</svg>

<style>
    .icon {
        display: block;
        flex-shrink: 0;
    }
</style>
