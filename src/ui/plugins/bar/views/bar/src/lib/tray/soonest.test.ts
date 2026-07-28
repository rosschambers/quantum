import { describe, it, expect } from 'vitest';
import { firesAtUnix, soonestActive, remainingSeconds } from './soonest';
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
  it('remainingSeconds never negative', () => {
    expect(remainingSeconds(mk('a', 'active', 40), 100)).toBe(0);
    expect(remainingSeconds(mk('a', 'active', 140), 100)).toBe(40);
  });
});
