<script lang="ts">
    /**
     * Hand-drawn SVG icons for the bar. Each icon is authored on a
     * 24x24 viewBox with its visual content centered in the box, so
     * scaling them down inside a Ring component yields a consistently
     * centered glyph regardless of which icon is in play. Stroke uses
     * currentColor so callers can drive color via CSS.
     *
     * Why not Nerd Font? Each Nerd Font codepoint encodes a glyph at
     * its own arbitrary position within the em-square, so the same
     * font-size produces a different visual size for each icon, and
     * the optical center shifts per-icon. SVG paths sidestep both.
     */
    interface Props {
        name: IconName;
        /** Width and height in px. Default 14. */
        size?: number;
        /** Stroke color override; otherwise inherits currentColor. */
        color?: string;
        /** Stroke width relative to the 24px viewBox. Default 2. */
        stroke?: number;
    }

    export type IconName =
        | 'sun'
        | 'wifi'
        | 'wifi_off'
        | 'ethernet'
        | 'battery_full'
        | 'battery_high'
        | 'battery_mid'
        | 'battery_low'
        | 'battery_empty'
        | 'battery_charging'
        | 'volume_high'
        | 'volume_low'
        | 'volume_off'
        | 'bluetooth'
        | 'leaf'
        | 'balanced'
        | 'bolt'
        | 'power'
        | 'cpu'
        | 'memory'
        | 'play'
        | 'pause'
        | 'prev'
        | 'next';

    let { name, size = 14, color = 'currentColor', stroke = 2 }: Props = $props();
</script>

<svg
    class="icon"
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke={color}
    stroke-width={stroke}
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
>
    {#if name === 'sun'}
        <circle cx="12" cy="12" r="4" />
        <line x1="12" y1="2" x2="12" y2="5" />
        <line x1="12" y1="19" x2="12" y2="22" />
        <line x1="2" y1="12" x2="5" y2="12" />
        <line x1="19" y1="12" x2="22" y2="12" />
        <line x1="4.93" y1="4.93" x2="7.05" y2="7.05" />
        <line x1="16.95" y1="16.95" x2="19.07" y2="19.07" />
        <line x1="4.93" y1="19.07" x2="7.05" y2="16.95" />
        <line x1="16.95" y1="7.05" x2="19.07" y2="4.93" />
    {:else if name === 'wifi'}
        <path d="M3.5 9 Q12 1.5 20.5 9" />
        <path d="M6.5 13 Q12 7.5 17.5 13" />
        <path d="M9.5 17 Q12 14 14.5 17" />
        <circle cx="12" cy="20" r="0.8" fill={color} stroke="none" />
    {:else if name === 'wifi_off'}
        <line x1="3" y1="3" x2="21" y2="21" />
        <path d="M3.5 9 Q9 4.5 13 5" />
        <circle cx="12" cy="20" r="0.8" fill={color} stroke="none" />
    {:else if name === 'ethernet'}
        <rect x="3" y="9" width="18" height="11" rx="2" />
        <line x1="7" y1="9" x2="7" y2="6" />
        <line x1="12" y1="9" x2="12" y2="4" />
        <line x1="17" y1="9" x2="17" y2="6" />
        <line x1="7" y1="20" x2="7" y2="23" />
        <line x1="12" y1="20" x2="12" y2="23" />
        <line x1="17" y1="20" x2="17" y2="23" />
    {:else if name === 'battery_full' || name === 'battery_high' || name === 'battery_mid' || name === 'battery_low' || name === 'battery_empty'}
        <rect x="5" y="5" width="14" height="16" rx="1.5" />
        <rect x="9" y="3" width="6" height="2" rx="0.5" fill={color} stroke="none" />
        {#if name === 'battery_full'}
            <rect x="7" y="7" width="10" height="12" rx="0.5" fill={color} stroke="none" />
        {:else if name === 'battery_high'}
            <rect x="7" y="10" width="10" height="9" rx="0.5" fill={color} stroke="none" />
        {:else if name === 'battery_mid'}
            <rect x="7" y="13" width="10" height="6" rx="0.5" fill={color} stroke="none" />
        {:else if name === 'battery_low'}
            <rect x="7" y="16" width="10" height="3" rx="0.5" fill={color} stroke="none" />
        {/if}
    {:else if name === 'battery_charging'}
        <rect x="5" y="5" width="14" height="16" rx="1.5" />
        <rect x="9" y="3" width="6" height="2" rx="0.5" fill={color} stroke="none" />
        <polygon points="13,7 9,14 12,14 10,19 16,11 13,11" fill={color} stroke="none" />
    {:else if name === 'volume_high'}
        <polygon points="4,9 8,9 13,5 13,19 8,15 4,15" fill={color} stroke="none" />
        <path d="M16 8.5 Q18 12 16 15.5" />
        <path d="M18.5 5.5 Q22 12 18.5 18.5" />
    {:else if name === 'volume_low'}
        <polygon points="4,9 8,9 13,5 13,19 8,15 4,15" fill={color} stroke="none" />
        <path d="M16 8.5 Q18 12 16 15.5" />
    {:else if name === 'volume_off'}
        <polygon points="4,9 8,9 13,5 13,19 8,15 4,15" fill={color} stroke="none" />
        <line x1="17" y1="8" x2="22" y2="16" />
        <line x1="22" y1="8" x2="17" y2="16" />
    {:else if name === 'bluetooth'}
        <polyline points="7,7 17,17 12,21 12,3 17,7 7,17" />
    {:else if name === 'leaf'}
        <path d="M20 4 Q4 4 4 14 Q4 20 10 20 Q20 20 20 4 Z" />
        <path d="M4 20 L14 10" />
    {:else if name === 'balanced'}
        <line x1="4" y1="12" x2="20" y2="12" />
        <polyline points="15,7 20,12 15,17" />
    {:else if name === 'bolt'}
        <polygon points="13,2 4,14 11,14 9,22 20,10 13,10 15,2" fill={color} stroke="none" />
    {:else if name === 'power'}
        <line x1="12" y1="3" x2="12" y2="12" />
        <path d="M7.5 6 Q3 10 4 15 Q6 22 12 21 Q18 22 20 15 Q21 10 16.5 6" />
    {:else if name === 'cpu'}
        <rect x="6" y="6" width="12" height="12" rx="1.5" />
        <rect x="9" y="9" width="6" height="6" rx="0.5" fill={color} stroke="none" />
        <line x1="12" y1="2" x2="12" y2="5" />
        <line x1="12" y1="19" x2="12" y2="22" />
        <line x1="2" y1="12" x2="5" y2="12" />
        <line x1="19" y1="12" x2="22" y2="12" />
    {:else if name === 'memory'}
        <rect x="3" y="8" width="18" height="9" rx="1" />
        <line x1="7" y1="11" x2="7" y2="14" />
        <line x1="11" y1="11" x2="11" y2="14" />
        <line x1="15" y1="11" x2="15" y2="14" />
        <line x1="19" y1="11" x2="19" y2="14" />
    {:else if name === 'play'}
        <polygon points="7,4 19,12 7,20" fill={color} stroke="none" />
    {:else if name === 'pause'}
        <rect x="6" y="5" width="4" height="14" rx="0.5" fill={color} stroke="none" />
        <rect x="14" y="5" width="4" height="14" rx="0.5" fill={color} stroke="none" />
    {:else if name === 'prev'}
        <polygon points="17,5 7,12 17,19" fill={color} stroke="none" />
        <rect x="4" y="5" width="2" height="14" fill={color} stroke="none" />
    {:else if name === 'next'}
        <polygon points="7,5 17,12 7,19" fill={color} stroke="none" />
        <rect x="18" y="5" width="2" height="14" fill={color} stroke="none" />
    {/if}
</svg>

<style>
    .icon {
        display: block;
        flex-shrink: 0;
    }
</style>
