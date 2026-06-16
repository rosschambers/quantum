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

/** The change descriptor carried alongside every notifications snapshot. */
export interface NotificationChange {
  type: 'created' | 'updated' | 'dismissed';
  data: { id: number; timeout_ms: number };
}

/** The full envelope published on `notifications.event`. */
export interface NotificationEnvelope {
  change: NotificationChange;
  notifications: PendingNotification[];
}

/** A callback-based store over the current notification snapshot. */
export interface NotificationStore {
  subscribe(callback: (notifications: PendingNotification[]) => void): () => void;
}

const NOTIFICATION_CHANNEL = 'notifications.event';

/**
 * Create a notification store backed by the `notifications.event` stream.
 *
 * The server is the source of truth: every event carries the full current
 * snapshot, so consumers replace their state with `notifications` on each call.
 * Consumers derive a count via `notifications.length`.
 */
export function createNotificationStore(client: Client): NotificationStore {
  return {
    subscribe(callback: (notifications: PendingNotification[]) => void): () => void {
      return client.subscribe(NOTIFICATION_CHANNEL, (payload: unknown) => {
        const envelope = parseEnvelope(payload);
        if (envelope === null) {
          return;
        }
        callback(envelope.notifications);
      });
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
