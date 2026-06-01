/**
 * Icon-name pickers for the bar indicators. Each function returns
 * the name of an SVG icon defined in `Icon.svelte`. Centralising
 * the mapping keeps the indicators dumb and lets the visual
 * vocabulary evolve in one place.
 *
 * Why not Nerd Font codepoints anymore? Each font glyph has its
 * own arbitrary bbox position within its em-square, which made
 * visual centering inside the percentage rings inconsistent. The
 * SVG icons in `Icon.svelte` use a uniform 24x24 viewBox with
 * content drawn centered.
 */
import type { IconName } from './Icon.svelte';

export function batteryIcon(percent: number | null, charging: boolean): IconName {
    if (charging) return 'battery_charging';
    if (percent === null) return 'battery_empty';
    if (percent >= 80) return 'battery_full';
    if (percent >= 55) return 'battery_high';
    if (percent >= 30) return 'battery_mid';
    if (percent >= 10) return 'battery_low';
    return 'battery_empty';
}

export function volumeIcon(percent: number, muted: boolean): IconName {
    if (muted || percent === 0) return 'volume_off';
    if (percent < 50) return 'volume_low';
    return 'volume_high';
}

export function networkIcon(kind: string | null, hasPrimary: boolean): IconName {
    if (!hasPrimary) return 'wifi_off';
    if (kind === 'ethernet') return 'ethernet';
    return 'wifi';
}

export function powerProfileIcon(profile: string | null): IconName {
    if (profile === 'power_saver') return 'leaf';
    if (profile === 'performance') return 'bolt';
    return 'balanced';
}
