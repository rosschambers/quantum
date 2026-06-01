/**
 * Smooth cool-to-hot gradient. 0 maps to a cool blue, 100 maps to a
 * hot red, with a yellow-ish midpoint coming naturally from linear
 * interpolation across all three channels.
 *
 * The input is a "magnitude" — what color expresses about the value
 * is "how much" / "how loaded". Used directly by CPU, memory,
 * brightness, and volume, where more = hotter color.
 *
 * For indicators where a LOW reading is alarming (battery getting
 * empty, wifi signal getting weak), call `inverseGradientColor()`
 * instead, which flips the input so 100% = cool blue and 0% = red.
 * Same gradient stops; just reversed semantics.
 */
export function gradientColor(percent: number | null): string {
    const p = percent === null ? 0 : Math.max(0, Math.min(100, percent));
    const r = Math.round(80 + (p / 100) * 175);
    const g = Math.round(180 - (p / 100) * 130);
    const b = Math.round(250 - (p / 100) * 200);
    return `rgb(${r}, ${g}, ${b})`;
}

/**
 * Gradient with badness semantics: 100 = cool blue (good, full
 * battery / strong signal), 0 = red (bad, near-empty / no signal).
 * Implemented as `gradientColor(100 - percent)` so the visual
 * gradient is identical to the magnitude one, only the input axis
 * is flipped.
 */
export function inverseGradientColor(percent: number | null): string {
    if (percent === null) return gradientColor(null);
    return gradientColor(100 - percent);
}
