import type { TimeFormat } from "@quantum/client";

/** Linear interpolation between `a` and `b` by `t` in [0, 1]. */
function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** Clamp `value` into the inclusive range [min, max]. */
function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

/**
 * The open fraction of a timer still remaining, clamped to [0, 1].
 *
 * `(firesAtUnix - nowUnix) / totalSecs`. A timer that has already passed
 * returns 0; one with more time left than its recorded total clamps to 1.
 * A non-positive `totalSecs` (unknown or invalid duration) returns 0.
 */
export function remainingFraction(
  nowUnix: number,
  firesAtUnix: number,
  totalSecs: number,
): number {
  if (totalSecs <= 0) return 0;
  return clamp((firesAtUnix - nowUnix) / totalSecs, 0, 1);
}

/**
 * The fraction to draw. When `fill` is set the visual grows from empty toward
 * full as time elapses, so the displayed fraction is the inverse.
 */
export function displayFraction(frac: number, fill: boolean): number {
  return fill ? 1 - frac : frac;
}

/**
 * The stroke colour for a timer at fraction `frac`.
 *
 * Above `threshold` percent remaining the colour stays at the base `hue`.
 * At or below it the hue ramps linearly toward 0 (red) as the fraction falls
 * to zero. Saturation and lightness are fixed at 70% / 60% to match the
 * playground.
 */
export function rampColor(frac: number, hue: number, threshold: number): string {
  const rampFrac = threshold / 100;
  let finalHue = hue;
  if (rampFrac > 0 && frac <= rampFrac) {
    const t = 1 - frac / rampFrac;
    finalHue = lerp(hue, 0, t);
  }
  return `hsl(${finalHue} 70% 60%)`;
}

/**
 * Format the remaining time for display.
 *
 * `clock` renders `m:ss` (or `h:mm:ss` past an hour); `compact` renders a
 * single rounded unit (`12m` / `45s`); `percent` renders the true remaining
 * `frac` as a whole percentage. Seconds are ceilinged and never negative.
 */
export function formatTime(
  remainingSecs: number,
  fmt: TimeFormat,
  frac: number,
): string {
  if (fmt === "percent") {
    return `${Math.round(frac * 100)}%`;
  }
  const seconds = Math.max(0, Math.ceil(remainingSecs));
  if (fmt === "compact") {
    if (seconds >= 60) return `${Math.round(seconds / 60)}m`;
    return `${seconds}s`;
  }
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  if (hours) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  }
  return `${minutes}:${String(secs).padStart(2, "0")}`;
}
