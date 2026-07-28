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

/** Tracks each timer's max-seen remaining as its drain denominator. */
export class RingTotals {
  private totals = new Map<string, number>();

  fraction(timerId: string, remaining: number): number {
    if (remaining <= 0) return 0;
    const previous = this.totals.get(timerId) ?? 0;
    const total = Math.max(previous, remaining);
    this.totals.set(timerId, total);
    return Math.max(0, Math.min(1, remaining / total));
  }

  forget(timerId: string): void {
    this.totals.delete(timerId);
  }
}
