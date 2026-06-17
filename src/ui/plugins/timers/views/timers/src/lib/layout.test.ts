import { describe, it, expect } from "vitest";
import { defaultScatterPosition, clampScatterPosition } from "./layout";

describe("defaultScatterPosition", () => {
  it("places the first timer at the top-left inset", () => {
    expect(defaultScatterPosition(0)).toEqual({ x: 40, y: 40 });
  });

  it("spreads subsequent timers across a row then wraps", () => {
    expect(defaultScatterPosition(1)).toEqual({ x: 230, y: 40 });
    expect(defaultScatterPosition(2)).toEqual({ x: 420, y: 40 });
    // Fourth timer (index 3) wraps to the next row.
    expect(defaultScatterPosition(3)).toEqual({ x: 40, y: 230 });
  });

  it("is deterministic for the same index", () => {
    expect(defaultScatterPosition(5)).toEqual(defaultScatterPosition(5));
  });
});

describe("clampScatterPosition", () => {
  it("keeps a position inside the bounds untouched", () => {
    expect(clampScatterPosition(100, 80, 800, 600)).toEqual({ x: 100, y: 80 });
  });

  it("clamps negative coordinates to zero", () => {
    expect(clampScatterPosition(-30, -10, 800, 600)).toEqual({ x: 0, y: 0 });
  });

  it("clamps coordinates that exceed the surface minus the pad", () => {
    // Default pad is 60, so the max x is width - 60.
    expect(clampScatterPosition(900, 700, 800, 600)).toEqual({ x: 740, y: 540 });
  });
});
