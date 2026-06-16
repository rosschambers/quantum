import { describe, it, expect } from 'vitest';
import { createClient } from '../index';
import { createMockTransport } from '../transport';
import { createNotificationStore, type PendingNotification } from '../notifications';

const NOTIFICATION_CHANNEL = 'notifications.event';

function makeNotification(overrides: Partial<PendingNotification> = {}): PendingNotification {
  return {
    id: 1,
    app_name: 'Spotify',
    summary: 'Now playing',
    body: 'Song',
    icon: 'spotify',
    urgency: 'normal',
    timeout_ms: 5000,
    actions: [['default', 'Open']],
    ...overrides,
  };
}

describe('createNotificationStore', () => {
  it('emits the full notifications snapshot from a created envelope', () => {
    const transport = createMockTransport();
    const client = createClient({ transport });
    const store = createNotificationStore(client);

    const received: PendingNotification[][] = [];
    store.subscribe((notifications) => received.push(notifications));

    const first = makeNotification({ id: 1 });
    const second = makeNotification({
      id: 2,
      app_name: 'Slack',
      summary: 'New message',
      body: 'Hello',
      icon: null,
      urgency: 'critical',
      actions: [],
    });

    transport.notify({
      channel: NOTIFICATION_CHANNEL,
      payload: {
        change: { type: 'created', data: { id: 2, timeout_ms: 5000 } },
        notifications: [first, second],
      },
    });

    expect(received).toHaveLength(1);
    expect(received[0]).toHaveLength(2);
    expect(received[0][0]).toEqual(first);
    expect(received[0][1]).toEqual(second);
  });

  it('replaces state on a dismissed envelope rather than appending', () => {
    const transport = createMockTransport();
    const client = createClient({ transport });
    const store = createNotificationStore(client);

    const received: PendingNotification[][] = [];
    store.subscribe((notifications) => received.push(notifications));

    transport.notify({
      channel: NOTIFICATION_CHANNEL,
      payload: {
        change: { type: 'created', data: { id: 2, timeout_ms: 5000 } },
        notifications: [makeNotification({ id: 1 }), makeNotification({ id: 2 })],
      },
    });

    transport.notify({
      channel: NOTIFICATION_CHANNEL,
      payload: {
        change: { type: 'dismissed', data: { id: 1, timeout_ms: 0 } },
        notifications: [makeNotification({ id: 2 })],
      },
    });

    expect(received).toHaveLength(2);
    expect(received[1]).toHaveLength(1);
    expect(received[1][0].id).toBe(2);
  });

  it('parses string payloads defensively', () => {
    const transport = createMockTransport();
    const client = createClient({ transport });
    const store = createNotificationStore(client);

    const received: PendingNotification[][] = [];
    store.subscribe((notifications) => received.push(notifications));

    transport.notify({
      channel: NOTIFICATION_CHANNEL,
      payload: JSON.stringify({
        change: { type: 'created', data: { id: 1, timeout_ms: 5000 } },
        notifications: [makeNotification({ id: 1 })],
      }),
    });

    expect(received).toHaveLength(1);
    expect(received[0]).toHaveLength(1);
    expect(received[0][0].id).toBe(1);
  });

  it('does not reference window anywhere in its data flow', () => {
    const transport = createMockTransport();
    const client = createClient({ transport });
    const store = createNotificationStore(client);

    const received: PendingNotification[][] = [];
    store.subscribe((notifications) => received.push(notifications));

    const originalWindow = (globalThis as { window?: unknown }).window;
    // Ensure the store never touches a global window object.
    (globalThis as { window?: unknown }).window = undefined;
    try {
      transport.notify({
        channel: NOTIFICATION_CHANNEL,
        payload: {
          change: { type: 'created', data: { id: 1, timeout_ms: 5000 } },
          notifications: [makeNotification({ id: 1 })],
        },
      });
    } finally {
      (globalThis as { window?: unknown }).window = originalWindow;
    }

    expect(received).toHaveLength(1);
    expect(received[0][0].id).toBe(1);
  });

  it('unsubscribe stops further callbacks', () => {
    const transport = createMockTransport();
    const client = createClient({ transport });
    const store = createNotificationStore(client);

    const received: PendingNotification[][] = [];
    const unsubscribe = store.subscribe((notifications) => received.push(notifications));

    transport.notify({
      channel: NOTIFICATION_CHANNEL,
      payload: {
        change: { type: 'created', data: { id: 1, timeout_ms: 5000 } },
        notifications: [makeNotification({ id: 1 })],
      },
    });
    expect(received).toHaveLength(1);

    unsubscribe();

    transport.notify({
      channel: NOTIFICATION_CHANNEL,
      payload: {
        change: { type: 'dismissed', data: { id: 1, timeout_ms: 0 } },
        notifications: [],
      },
    });
    expect(received).toHaveLength(1);
  });
});
