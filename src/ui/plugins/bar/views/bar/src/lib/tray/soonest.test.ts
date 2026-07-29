import { describe, it, expect } from 'vitest';
import { firesAtUnix, soonestActive, remainingSeconds, ringTarget, RingTotals } from './soonest';
import type { Timer } from '@quantum/client';

function mk(id: string, status: 'active' | 'expired', fires: number): Timer {
  return {
    id, label: id, status,
    kind: { type: 'one_shot', end_unix: fires },
    visual: {} as any, notify: {} as any, scatter_pos: null,
  };
}

describe('soonest', () => {
  it('firesAtUnix reads one_shot end and recurring next_fire', () => {
    expect(firesAtUnix(mk('a', 'active', 100))).toBe(100);
    const rec = { ...mk('b', 'active', 0), kind: { type: 'recurring', days: [], time: { hour: 0, minute: 0 }, next_fire_unix: 250 } } as Timer;
    expect(firesAtUnix(rec)).toBe(250);
  });
  it('soonestActive picks smallest fires among active only', () => {
    const timers = [mk('a', 'active', 300), mk('b', 'expired', 50), mk('c', 'active', 120)];
    expect(soonestActive(timers, 0)?.id).toBe('c');
  });
  it('soonestActive returns null when none active', () => {
    expect(soonestActive([mk('a', 'expired', 10)], 0)).toBeNull();
  });
  it('soonestActive breaks ties in favor of the first in array order', () => {
    const timers = [mk('first', 'active', 200), mk('second', 'active', 200)];
    expect(soonestActive(timers, 0)?.id).toBe('first');
  });
  it('remainingSeconds never negative', () => {
    expect(remainingSeconds(mk('a', 'active', 40), 100)).toBe(0);
    expect(remainingSeconds(mk('a', 'active', 140), 100)).toBe(40);
  });
});

describe('ringTarget', () => {
  it('returns null when there are no timers', () => {
    expect(ringTarget([], 0)).toBeNull();
  });
  it('shows the soonest active timer, not fired, when only active timers exist', () => {
    const result = ringTarget([mk('a', 'active', 300), mk('c', 'active', 120)], 0);
    expect(result?.timer.id).toBe('c');
    expect(result?.fired).toBe(false);
  });
  it('shows a fired ring for an expired timer even while another is still active', () => {
    // fired-wins: a fired timer must not be missed, so it takes priority over a
    // still-counting active timer.
    const result = ringTarget([mk('active', 'active', 500), mk('done', 'expired', 10)], 100);
    expect(result?.timer.id).toBe('done');
    expect(result?.fired).toBe(true);
  });
  it('among several expired timers, targets the one that fired soonest', () => {
    const result = ringTarget([mk('later', 'expired', 90), mk('earlier', 'expired', 30)], 100);
    expect(result?.timer.id).toBe('earlier');
    expect(result?.fired).toBe(true);
  });
  it('reverts to the active countdown once the expired timer is gone', () => {
    const result = ringTarget([mk('a', 'active', 300)], 0);
    expect(result?.timer.id).toBe('a');
    expect(result?.fired).toBe(false);
  });
});

describe('RingTotals', () => {
  it('first sighting is full, then drains', () => {
    const totals = new RingTotals();
    expect(totals.fraction('a', 300)).toBe(1);
    expect(totals.fraction('a', 150)).toBeCloseTo(0.5, 5);
    expect(totals.fraction('a', 0)).toBe(0);
  });
  it('a larger later remaining raises the total (re-armed timer)', () => {
    const totals = new RingTotals();
    totals.fraction('a', 100);
    expect(totals.fraction('a', 400)).toBe(1);
  });
  it('forget resets', () => {
    const totals = new RingTotals();
    totals.fraction('a', 100);
    totals.forget('a');
    expect(totals.fraction('a', 50)).toBe(1);
  });
});
