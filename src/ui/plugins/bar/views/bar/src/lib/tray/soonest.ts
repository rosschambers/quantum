import type { Timer } from '@quantum/client';

/** The absolute unix time a timer fires: one_shot end, or recurring next fire. */
export function firesAtUnix(timer: Timer): number {
  return timer.kind.type === 'one_shot' ? timer.kind.end_unix : timer.kind.next_fire_unix;
}

/** The active timer that fires soonest, or null when none are active. */
export function soonestActive(timers: Timer[], nowUnix: number): Timer | null {
  let best: Timer | null = null;
  for (const timer of timers) {
    if (timer.status !== 'active') continue;
    if (best === null || firesAtUnix(timer) < firesAtUnix(best)) best = timer;
  }
  return best;
}

/** Seconds remaining until a timer fires, clamped at zero. */
export function remainingSeconds(timer: Timer, nowUnix: number): number {
  return Math.max(0, firesAtUnix(timer) - nowUnix);
}
