import { describe, it, expect } from 'vitest';
import { firesAtUnix, soonestActive, remainingSeconds, RingTotals } from './soonest';
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
