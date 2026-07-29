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

/** The soonest-firing timer among a subset, or null when the subset is empty. */
function soonestOf(timers: Timer[]): Timer | null {
  let best: Timer | null = null;
  for (const timer of timers) {
    if (best === null || firesAtUnix(timer) < firesAtUnix(best)) best = timer;
  }
  return best;
}

/** What the bar ring should show: a timer and whether it is in the fired state. */
export interface RingTarget {
  timer: Timer;
  fired: boolean;
}

/**
 * Choose the timer the bar ring represents, and whether it is fired.
 *
 * Fired-wins precedence: an expired timer still present in the store must not
 * be missed, so it takes priority over any still-counting active timer. When
 * no expired timer is present, the ring shows the soonest active timer's
 * draining countdown. Returns null when there are no timers at all.
 */
export function ringTarget(timers: Timer[], nowUnix: number): RingTarget | null {
  const expired = soonestOf(timers.filter((timer) => timer.status === 'expired'));
  if (expired !== null) return { timer: expired, fired: true };
  const active = soonestActive(timers, nowUnix);
  if (active !== null) return { timer: active, fired: false };
  return null;
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
