import { describe, it, expect, vi } from 'vitest';
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

  it('fetches the current snapshot via provider.query on subscribe', async () => {
    // A freshly opened consumer (the notification center) must catch up to the
    // current notification list immediately, not wait for the next change
    // event. It queries the provider on subscribe, exactly like every other
    // tray indicator.
    const existing = makeNotification({ id: 9, app_name: 'Discord' });
    const client = {
      call: vi.fn().mockResolvedValue({
        change: null,
        notifications: [existing],
      }),
      subscribe: vi.fn().mockReturnValue(vi.fn()),
    };

    const received: PendingNotification[][] = [];
    createNotificationStore(client).subscribe((notifications) => received.push(notifications));

    // Flush the resolved provider.query promise.
    await Promise.resolve();
    await Promise.resolve();

    expect(client.call).toHaveBeenCalledWith('provider.query', { id: 'notifications' });
    expect(client.subscribe).toHaveBeenCalledWith(NOTIFICATION_CHANNEL, expect.any(Function));
    expect(received).toHaveLength(1);
    expect(received[0]).toHaveLength(1);
    expect(received[0][0].id).toBe(9);
  });

  it('retries the catch-up query when the first attempt rejects', async () => {
    // A transient provider.query rejection during a busy startup must not leave
    // the center permanently empty: the store retries and delivers once a later
    // attempt succeeds.
    vi.useFakeTimers();
    const existing = makeNotification({ id: 7, app_name: 'Discord' });
    const call = vi
      .fn()
      .mockRejectedValueOnce(new Error('timed out'))
      .mockResolvedValue({ change: null, notifications: [existing] });
    const client = {
      call,
      subscribe: vi.fn().mockReturnValue(vi.fn()),
    };

    const received: PendingNotification[][] = [];
    createNotificationStore(client).subscribe((notifications) => received.push(notifications));

    // Let the first (rejected) query settle, then advance past the retry delay
    // and let the second (resolved) query settle.
    await Promise.resolve();
    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(250);
    await Promise.resolve();
    await Promise.resolve();

    expect(call).toHaveBeenCalledTimes(2);
    expect(received).toHaveLength(1);
    expect(received[0][0].id).toBe(7);
    vi.useRealTimers();
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
