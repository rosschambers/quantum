import type { Point } from "@quantum/client";

/** Inset of the first scatter timer from the top-left corner, in pixels. */
const SCATTER_INSET = 40;
/** Horizontal step between scatter timers within a row, in pixels. */
const SCATTER_STEP = 190;
/** Number of timers per scatter row before wrapping. */
const SCATTER_COLUMNS = 3;
/** Default drag pad: how far a timer's top-left must stay from the far edge. */
const SCATTER_PAD = 60;

/**
 * A deterministic default scatter placement for the timer at `index`.
 *
 * Timers march left to right with a fixed step, wrapping to a new row every
 * three timers. Ported from the playground so positions are stable until the
 * user drags a timer and persists its `scatter_pos`.
 */
export function defaultScatterPosition(index: number): Point {
  const column = index % SCATTER_COLUMNS;
  const row = Math.floor(index / SCATTER_COLUMNS);
  return {
    x: SCATTER_INSET + column * SCATTER_STEP,
    y: SCATTER_INSET + row * SCATTER_STEP,
  };
}

/**
 * Clamp a dragged scatter position so the timer stays on the surface.
 *
 * `x` is held in [0, width - pad] and `y` in [0, height - pad], matching the
 * playground's drag bounds so a timer cannot be dragged fully off-screen.
 */
export function clampScatterPosition(
  x: number,
  y: number,
  width: number,
  height: number,
  pad: number = SCATTER_PAD,
): Point {
  return {
    x: Math.max(0, Math.min(width - pad, x)),
    y: Math.max(0, Math.min(height - pad, y)),
  };
}
