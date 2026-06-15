import { Store, type WritableStore } from 'svelte/store';
import type { Notification } from './generated';

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

export interface NotificationState {
  notifications: PendingNotification[];
  expandedId: number | null;
  widgetExpanded: boolean;
}

// Create and register a notification event channel in the bridge.
let nextId = 0;

const NOTIFICATION_CHANNEL = 'notifications.event';

/**
 * Subscribe to new/updated/dismissed notifications via the quantum bridge.
 */
export function subscribeNotifications(cb: (notifications: PendingNotification[]) => void): () => void {
  // Use the existing client channel subscription.
  const cleanup = window.__quantum_notify!(NOTIFICATION_CHANNEL, (payload: string) => {
    try {
      const data = JSON.parse(payload);
      if (data.type === 'created') {
        const notif = data.data;
        nextId++;
        const p: PendingNotification = {
          id: notif.id || nextId,
          app_name: notif.app_name || 'Unknown',
          summary: notif.summary || '',
          body: notif.body || '',
          icon: notif.icon || null,
          urgency: (notif.urgency as PendingNotification['urgency']) ?? 'normal',
          timeout_ms: notif.timeout_ms || 0,
          actions: notif.actions || [],
        };
        cb([...notifications, p]);
      }
    } catch {
      // Silently handle parse errors.
    }
  });

  return cleanup;
}

const notifications: PendingNotification[] = [];

// Create a simple writable store wrapper (not Svelte Store since we don't have @quantum/client yet).
export const notificationStore: WritableStore<PendingNotification[]> = {
  set(value: PendingNotification[]) {
    notifications.length = 0;
    for (const n of value) notifications.push(n);
  },
  subscribe(cb: (value: PendingNotification[]) => void): () => void {
    cb(notifications);
    const cleanup = subscribeNotifications(cb);
    return cleanup;
  },
};

export const notificationCount: WritableStore<number> = {
  set(value: number) {},
  subscribe(cb: (value: number) => void): () => void {
    cb(notifications.length);
    const unsubscribe = notificationStore.subscribe(() => cb(notifications.length));
    return unsubscribe;
  },
};
