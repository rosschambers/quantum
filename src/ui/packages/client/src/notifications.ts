import type { Client } from './index';

/**
 * A single pending notification as published by the Rust notifications provider
 * on the `notifications.event` channel.
 */
export interface PendingNotification {
  id: number;
  app_name: string;
  summary: string;
  body: string;
  icon: string | null;
  urgency: 'low' | 'normal' | 'critical';
  timeout_ms: number;
  actions: Array<[string, string]>;
}

/**
 * The change descriptor carried alongside every notifications snapshot.
 *
 * `toasts_cleared` is a display-only signal (the user opened the notification
 * center): the transient toasts should dismiss while the stored notifications
 * are unchanged, so it carries no `data`.
 */
export interface NotificationChange {
  type: 'created' | 'updated' | 'dismissed' | 'toasts_cleared';
  data?: { id: number; timeout_ms: number };
}

/**
 * The full envelope published on `notifications.event`.
 *
 * `change` is null for the initial catch-up snapshot the provider emits on
 * subscribe (and returns from `provider.query`); it carries a change
 * descriptor for every subsequent live event.
 */
export interface NotificationEnvelope {
  change: NotificationChange | null;
  notifications: PendingNotification[];
}

/** A callback-based store over the current notification snapshot. */
export interface NotificationStore {
  /**
   * `callback` receives the current notification list on every snapshot.
   * The optional `onChange` receives the change descriptor (null for the
   * initial catch-up), letting a consumer react to display-only signals such
   * as `toasts_cleared` without affecting the list it renders.
   */
  subscribe(
    callback: (notifications: PendingNotification[]) => void,
    onChange?: (change: NotificationChange | null) => void,
  ): () => void;
}

const NOTIFICATION_CHANNEL = 'notifications.event';
const NOTIFICATION_PROVIDER = 'notifications';

// How many times the initial catch-up `provider.query` is retried, and the
// delay between attempts. The query can transiently reject or time out during
// a busy daemon startup; without a retry the notification center would render
// permanently empty until the next brand-new notification arrived.
const CATCH_UP_RETRIES = 3;
const CATCH_UP_RETRY_DELAY_MS = 250;

/** The subset of the client surface the notification store needs. */
type NotificationClient = Pick<Client, 'call' | 'subscribe'>;

/**
 * Create a notification store backed by `provider.query` and the
 * `notifications.event` stream.
 *
 * On subscribe it immediately fetches the current snapshot via
 * `provider.query` and delivers it, so a freshly opened consumer (the
 * notification center) catches up to the bell's count instead of waiting for
 * the next change event. It then subscribes to `notifications.event`, where
 * every event carries the full current snapshot and replaces the consumer's
 * state. Consumers derive a count via `notifications.length`.
 */
export function createNotificationStore(client: NotificationClient): NotificationStore {
  return {
    subscribe(
      callback: (notifications: PendingNotification[]) => void,
      onChange?: (change: NotificationChange | null) => void,
    ): () => void {
      // Tracks whether the subscription is still live so a pending retry does
      // not fire a callback after teardown.
      let active = true;

      // Fetch the current snapshot, retrying a bounded number of times if the
      // call rejects. A delivered snapshot stops the retries; the live stream
      // below remains the source of truth for every subsequent change.
      const attemptCatchUp = (remaining: number): void => {
        if (!active) {
          return;
        }
        void client
          .call('provider.query', { id: NOTIFICATION_PROVIDER })
          .then((result) => {
            if (!active) {
              return;
            }
            const envelope = parseEnvelope(result);
            if (envelope !== null) {
              callback(envelope.notifications);
              onChange?.(envelope.change);
            }
          })
          .catch(() => {
            if (active && remaining > 0) {
              setTimeout(() => attemptCatchUp(remaining - 1), CATCH_UP_RETRY_DELAY_MS);
            }
          });
      };
      attemptCatchUp(CATCH_UP_RETRIES);

      const off = client.subscribe(NOTIFICATION_CHANNEL, (payload: unknown) => {
        const envelope = parseEnvelope(payload);
        if (envelope === null) {
          return;
        }
        callback(envelope.notifications);
        onChange?.(envelope.change);
      });

      return () => {
        active = false;
        off();
      };
    },
  };
}

/**
 * Parse a `notifications.event` payload into an envelope.
 *
 * Handles both already-parsed object payloads (the transport's normal case)
 * and raw JSON string payloads (defensive). Returns null on malformed input.
 */
function parseEnvelope(payload: unknown): NotificationEnvelope | null {
  let value: unknown = payload;

  if (typeof value === 'string') {
    try {
      value = JSON.parse(value);
    } catch {
      return null;
    }
  }

  if (
    value === null ||
    typeof value !== 'object' ||
    !Array.isArray((value as { notifications?: unknown }).notifications)
  ) {
    return null;
  }

  return value as NotificationEnvelope;
}
