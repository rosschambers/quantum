import { describe, it, expect, vi } from 'vitest';
import { createTimerStore, type TimerKind, type TimerStoreData } from './timer';

/** Narrowing helper that proves `TimerKind` discriminates on `type`. */
function fireTime(kind: TimerKind): number {
  if (kind.type === 'one_shot') {
    return kind.end_unix;
  }
  return kind.next_fire_unix;
}

const SAMPLE: TimerStoreData = {
  settings: {
    layout: 'scatter',
    gap: 24,
    align: 'top_right',
    defaults_visual: {
      style: 'mixed',
      size: 130,
      thickness: 12,
      fill: true,
      reverse: true,
      accent_hue: 220,
      track_opacity: 0,
      label_visibility: 'hover',
      time_visibility: 'hover',
      text_position: 'center',
      text_color: 'accent',
      time_format: 'compact',
      font_scale: 105,
      font_weight: 500,
      uppercase: true,
    },
    defaults_notify: {
      notification: true,
      sound: null,
      urgency_ramp: true,
      ramp_threshold: 20,
      pulse: true,
      flash: true,
    },
  },
  timers: [
    {
      id: '57759fba',
      label: 'Smoke Test',
      kind: { type: 'one_shot', end_unix: 1781663172 },
      visual: {
        style: 'mixed',
        size: 130,
        thickness: 12,
        fill: true,
        reverse: true,
        accent_hue: 220,
        track_opacity: 0,
        label_visibility: 'hover',
        time_visibility: 'hover',
        text_position: 'center',
        text_color: 'accent',
        time_format: 'compact',
        font_scale: 105,
        font_weight: 500,
        uppercase: true,
      },
      notify: {
        notification: true,
        sound: null,
        urgency_ramp: true,
        ramp_threshold: 20,
        pulse: true,
        flash: true,
      },
      status: 'expired',
      scatter_pos: null,
    },
  ],
};

describe('TimerKind discriminated union', () => {
  it('narrows one_shot to end_unix', () => {
    expect(fireTime({ type: 'one_shot', end_unix: 42 })).toBe(42);
  });

  it('narrows recurring to next_fire_unix', () => {
    expect(
      fireTime({
        type: 'recurring',
        days: ['tuesday', 'thursday'],
        time: { hour: 18, minute: 0 },
        next_fire_unix: 99,
      }),
    ).toBe(99);
  });
});

describe('createTimerStore', () => {
  it('fetches timer.list once on subscribe and delivers it', async () => {
    const unsubscribe = vi.fn();
    const client = {
      call: vi.fn().mockResolvedValue(SAMPLE),
      subscribe: vi.fn().mockReturnValue(unsubscribe),
    };

    const received: TimerStoreData[] = [];
    const store = createTimerStore(client);
    const teardown = store.subscribe((data) => received.push(data));

    // Allow the resolved timer.list promise to flush.
    await Promise.resolve();
    await Promise.resolve();

    expect(client.call).toHaveBeenCalledTimes(1);
    expect(client.call).toHaveBeenCalledWith('timer.list', {});
    expect(client.subscribe).toHaveBeenCalledWith('timer.event', expect.any(Function));
    expect(received[0]).toEqual(SAMPLE);

    teardown();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });

  it('delivers snapshots from timer.event', async () => {
    let channelCallback: ((payload: unknown) => void) | undefined;
    const client = {
      call: vi.fn().mockResolvedValue(SAMPLE),
      subscribe: vi.fn().mockImplementation((_channel: string, cb: (payload: unknown) => void) => {
        channelCallback = cb;
        return vi.fn();
      }),
    };

    const received: TimerStoreData[] = [];
    createTimerStore(client).subscribe((data) => received.push(data));

    await Promise.resolve();
    await Promise.resolve();

    channelCallback?.({ change: 'snapshot', settings: SAMPLE.settings, timers: SAMPLE.timers });

    const last = received[received.length - 1];
    expect(last.timers).toEqual(SAMPLE.timers);
    expect(last.settings).toEqual(SAMPLE.settings);
  });
});
