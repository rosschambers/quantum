import { describe, it, expect } from "vitest";
import { computeRingSpecs } from "./ripples";

describe("computeRingSpecs", () => {
  it("returns one spec per ring", () => {
    expect(computeRingSpecs(3, 700)).toHaveLength(3);
  });

  it("starts the first ring with no delay", () => {
    const specs = computeRingSpecs(3, 700);
    expect(specs[0].delayMs).toBe(0);
  });

  it("staggers delays strictly increasing", () => {
    const specs = computeRingSpecs(3, 700);
    for (let index = 1; index < specs.length; index += 1) {
      expect(specs[index].delayMs).toBeGreaterThan(specs[index - 1].delayMs);
    }
  });

  it("staggers each ring by duration times 0.16", () => {
    const durationMs = 700;
    const specs = computeRingSpecs(3, durationMs);
    expect(specs[1].delayMs).toBeCloseTo(durationMs * 0.16);
    expect(specs[2].delayMs).toBeCloseTo(2 * durationMs * 0.16);
  });
});
