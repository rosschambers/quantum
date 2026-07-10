/** Map a raw pactl profile name to a human-readable label. Bluetooth-style
 * A2DP/HSP/HFP names get the tradeoff-aware wording; `off` becomes "Disabled";
 * everything else falls back to the pactl-supplied description. */
export function friendlyProfileName(rawName: string, description: string): string {
    const name = rawName.toLowerCase();
    if (name === 'off') return 'Disabled';
    if (name.includes('a2dp')) return 'High quality (no mic)';
    if (
        name.includes('headset') ||
        name.includes('handsfree') ||
        name.includes('hsp') ||
        name.includes('hfp')
    ) {
        return 'Headset (mic on, lower quality)';
    }
    return description;
}

/** Short output/input count label for a profile. A profile with zero sinks
 * produces no sound, which is called out explicitly. */
export function profileCountLabel(sinkCount: number, sourceCount: number): string {
    if (sinkCount === 0) return '0 out = no sound';
    return `${sinkCount} out / ${sourceCount} in`;
}
