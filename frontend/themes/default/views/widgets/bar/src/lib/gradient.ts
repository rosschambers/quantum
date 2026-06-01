/**
 * Smooth cool-to-hot gradient. 0% maps to a cool blue, 100% maps to a
 * hot red, with a yellow-ish midpoint coming naturally from linear
 * interpolation across all three channels. Shared by every indicator
 * that wants to color a value by its magnitude so the entire bar reads
 * with a consistent visual language.
 */
export function gradientColor(percent: number | null): string {
    const p = percent === null ? 0 : Math.max(0, Math.min(100, percent));
    const r = Math.round(80 + (p / 100) * 175);
    const g = Math.round(180 - (p / 100) * 130);
    const b = Math.round(250 - (p / 100) * 200);
    return `rgb(${r}, ${g}, ${b})`;
}
