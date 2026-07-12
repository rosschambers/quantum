// Pure charting helpers for the task manager graph strip. The sparkline math is
// ported verbatim from the bar's `SystemMeters.svelte` (Catmull-Rom-to-Bezier)
// so the two read as the same metric; the only addition is an explicit `maxV`
// so a series can scale to something other than a fixed 0-100 (the network
// graph scales to its own rolling maximum). No charting library — a smoothed
// SVG `d` string is all the strip needs.

/** Maximum number of samples any rolling series retains. */
export const MAX_SAMPLES = 300;

/**
 * Append a sample to a rolling series, returning a NEW array capped at `cap`
 * (oldest samples fall off the left). Immutable by design: the graph strip
 * holds each series in `$state`, and returning a fresh array is what triggers
 * the derived path recomputation. Mirrors the `[...history, value].slice(-N)`
 * pattern SystemMeters uses. History starts empty at mount — there is no
 * backfill, so the graphs fill in from the moment the view opens.
 */
export function pushSample(series: number[], value: number, cap: number = MAX_SAMPLES): number[] {
    return [...series, value].slice(-cap);
}

/**
 * Smooth cool-to-hot gradient. 0% maps to a cool blue, 100% maps to a hot red,
 * with a yellow-ish midpoint falling out naturally from linear interpolation
 * across all three channels. Identical formula to SystemMeters' `gradientColor`
 * so the strip strokes match the bar meters. Values are clamped to 0-100.
 */
export function hotColdColor(pct: number): string {
    const p = Math.min(100, Math.max(0, pct));
    const r = Math.round(80 + (p / 100) * 175);
    const g = Math.round(180 - (p / 100) * 130);
    const b = Math.round(250 - (p / 100) * 200);
    return `rgb(${r}, ${g}, ${b})`;
}

/**
 * Build a smoothed SVG path `d` attribute from a samples array using a
 * Catmull-Rom-to-Bezier conversion (uniform, tension 0.5): for each pair
 * (p1, p2) the surrounding points (p0, p1, p2, p3) produce two cubic control
 * points cp1 = p1 + (p2 - p0) / 6 and cp2 = p2 - (p3 - p1) / 6, duplicating the
 * endpoints to avoid running off the array. Returns an empty string for fewer
 * than two samples. `maxV` sets the value that maps to the top of the drawable
 * area; the network graph passes its rolling maximum rather than a fixed 100.
 */
export function smoothPath(samples: number[], width: number, height: number, maxV: number): string {
    if (samples.length < 2) return '';
    const pad = 3;
    const drawable = height - pad * 2;
    const stepX = width / Math.max(1, samples.length - 1);
    const points: Array<[number, number]> = samples.map((value, index) => [
        index * stepX,
        pad + (1 - value / maxV) * drawable,
    ]);
    const segments: string[] = [`M${points[0][0].toFixed(2)},${points[0][1].toFixed(2)}`];
    for (let i = 0; i < points.length - 1; i += 1) {
        const p0 = points[i - 1] ?? points[i];
        const p1 = points[i];
        const p2 = points[i + 1];
        const p3 = points[i + 2] ?? p2;
        const cp1x = p1[0] + (p2[0] - p0[0]) / 6;
        const cp1y = p1[1] + (p2[1] - p0[1]) / 6;
        const cp2x = p2[0] - (p3[0] - p1[0]) / 6;
        const cp2y = p2[1] - (p3[1] - p1[1]) / 6;
        segments.push(
            `C${cp1x.toFixed(2)},${cp1y.toFixed(2)} ${cp2x.toFixed(2)},${cp2y.toFixed(2)} ${p2[0].toFixed(2)},${p2[1].toFixed(2)}`,
        );
    }
    return segments.join(' ');
}
