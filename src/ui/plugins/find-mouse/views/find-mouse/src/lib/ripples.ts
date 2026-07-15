/** Timing specification for a single sonar ripple ring. */
export interface RingSpec {
  /** Delay before this ring's animation starts, in milliseconds. */
  delayMs: number;
}

/**
 * Compute the animation timing for a stack of expanding ripple rings.
 *
 * Pure function: given a ring count and the base animation duration, returns
 * one spec per ring. Each ring is staggered by `durationMs * 0.16` so ring `i`
 * begins at `i * durationMs * 0.16`, producing the outward sonar cadence.
 */
export function computeRingSpecs(
  count: number,
  durationMs: number,
): RingSpec[] {
  const stagger = durationMs * 0.16;
  const specs: RingSpec[] = [];
  for (let index = 0; index < count; index += 1) {
    specs.push({ delayMs: index * stagger });
  }
  return specs;
}
