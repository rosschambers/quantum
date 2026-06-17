import type { Client } from './index';

/** Visual rendering style for a timer. Mirrors the Rust `VisualStyle` enum. */
export type VisualStyle = 'ring' | 'wedge' | 'pie' | 'dots' | 'bar' | 'mixed';

/** When text (label or time) is shown relative to the timer surface. */
export type TextVisibility = 'always' | 'hover' | 'hidden';

/** Where text sits relative to the timer body. */
export type TextPosition = 'below' | 'above' | 'center';

/** Colour treatment for timer text. */
export type TextColor = 'accent' | 'white' | 'muted';

/** How the remaining time is formatted. */
export type TimeFormat = 'clock' | 'compact' | 'percent';

/** A built-in completion sound. */
export type SoundName = 'complete' | 'bell' | 'chime' | 'alarm';

/** A day of the week. Mirrors the lowercase serde values of the Rust enum. */
export type Weekday =
  | 'monday'
  | 'tuesday'
  | 'wednesday'
  | 'thursday'
  | 'friday'
  | 'saturday'
  | 'sunday';

/** A wall-clock time of day for recurring timers. */
export interface TimeOfDay {
  hour: number;
  minute: number;
}

/** Per-timer visual configuration. */
export interface VisualConfig {
  style: VisualStyle;
  size: number;
  thickness: number;
  fill: boolean;
  reverse: boolean;
  accent_hue: number;
  track_opacity: number;
  label_visibility: TextVisibility;
  time_visibility: TextVisibility;
  text_position: TextPosition;
  text_color: TextColor;
  time_format: TimeFormat;
  font_scale: number;
  font_weight: number;
  uppercase: boolean;
}

/** Per-timer notification configuration. */
export interface NotifyConfig {
  notification: boolean;
  sound: SoundName | null;
  urgency_ramp: boolean;
  ramp_threshold: number;
  pulse: boolean;
  flash: boolean;
}

/**
 * The schedule shape of a timer. Discriminated union on `type`:
 * `one_shot` carries an absolute end time; `recurring` carries the weekly
 * schedule plus the next computed fire time.
 */
export type TimerKind =
  | { type: 'one_shot'; end_unix: number }
  | { type: 'recurring'; days: Weekday[]; time: TimeOfDay; next_fire_unix: number };

/** A scatter-layout placement, in surface-relative coordinates. */
export interface Point {
  x: number;
  y: number;
}

/** Lifecycle status of a timer. */
export type TimerStatus = 'active' | 'expired';

/** A single timer. */
export interface Timer {
  id: string;
  label: string;
  kind: TimerKind;
  visual: VisualConfig;
  notify: NotifyConfig;
  status: TimerStatus;
  scatter_pos: Point | null;
}

/** Global timer-surface settings plus the defaults applied to new timers. */
export interface TimerSettings {
  layout: string;
  gap: number;
  align: string;
  defaults_visual: VisualConfig;
  defaults_notify: NotifyConfig;
}

/** The full snapshot returned by `timer.list`. */
export interface TimerStoreData {
  settings: TimerSettings;
  timers: Timer[];
}

/** The envelope published on the `timer.event` channel. */
export interface TimerEnvelope {
  change: string;
  settings: TimerSettings;
  timers: Timer[];
}

const TIMER_CHANNEL = 'timer.event';

/** A callback-based store over the current timer snapshot. */
export interface TimerStore {
  subscribe(callback: (data: TimerStoreData) => void): () => void;
}

/** The subset of the client surface the timer store needs. */
type TimerClient = Pick<Client, 'call' | 'subscribe'>;

/**
 * Create a timer store backed by `timer.list` and the `timer.event` stream.
 *
 * On subscribe it immediately fetches the current snapshot via `timer.list`
 * and delivers it, then subscribes to `timer.event` so every subsequent
 * snapshot replaces the consumer's state. The returned function tears down
 * the channel subscription.
 */
export function createTimerStore(client: TimerClient): TimerStore {
  return {
    subscribe(callback: (data: TimerStoreData) => void): () => void {
      void client
        .call('timer.list', {})
        .then((result) => {
          callback(result as TimerStoreData);
        })
        .catch(() => {
          // Initial fetch failures are non-fatal; the stream will deliver the
          // next snapshot once it arrives.
        });

      return client.subscribe(TIMER_CHANNEL, (payload: unknown) => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const envelope = payload as any;
        callback({ settings: envelope.settings, timers: envelope.timers });
      });
    },
  };
}
