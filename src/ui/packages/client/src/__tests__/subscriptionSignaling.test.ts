import { describe, it, expect } from 'vitest';
import { createClient } from '../index';
import type { Transport, JsonRpcRequest, JsonRpcResponse, JsonRpcNotification } from '../transport';

/**
 * A transport that records every request sent, so the ref-counted
 * `bridge.subscribe` / `bridge.unsubscribe` signaling can be asserted.
 */
function createCapturingTransport(): Transport & { sent: JsonRpcRequest[] } {
  const sent: JsonRpcRequest[] = [];
  return {
    sent,
    send(request: JsonRpcRequest): void {
      sent.push(request);
    },
    onResponse(_callback: (response: JsonRpcResponse) => void): () => void {
      return () => {};
    },
    onNotification(_callback: (notification: JsonRpcNotification) => void): () => void {
      return () => {};
    },
  };
}

function subscribeCalls(sent: JsonRpcRequest[], method: string, channel: string): number {
  return sent.filter(
    (request) =>
      request.method === method &&
      (request.params as { channel?: string } | undefined)?.channel === channel,
  ).length;
}

describe('@quantum/client subscription signaling', () => {
  it('sends bridge.subscribe once when a channel gains its first callback', () => {
    const transport = createCapturingTransport();
    const client = createClient({ transport });

    client.subscribe('processes.event', () => {});
    client.subscribe('processes.event', () => {});

    expect(subscribeCalls(transport.sent, 'bridge.subscribe', 'processes.event')).toBe(1);
  });

  it('does not send bridge.unsubscribe while other callbacks remain', () => {
    const transport = createCapturingTransport();
    const client = createClient({ transport });

    const first = client.subscribe('processes.event', () => {});
    client.subscribe('processes.event', () => {});

    first();

    expect(subscribeCalls(transport.sent, 'bridge.unsubscribe', 'processes.event')).toBe(0);
  });

  it('sends bridge.unsubscribe once when the last callback is removed', () => {
    const transport = createCapturingTransport();
    const client = createClient({ transport });

    const first = client.subscribe('processes.event', () => {});
    const second = client.subscribe('processes.event', () => {});

    first();
    second();

    expect(subscribeCalls(transport.sent, 'bridge.unsubscribe', 'processes.event')).toBe(1);
  });

  it('re-sends bridge.subscribe after the channel was fully unsubscribed and subscribed again', () => {
    const transport = createCapturingTransport();
    const client = createClient({ transport });

    const first = client.subscribe('mpris.event', () => {});
    first();
    client.subscribe('mpris.event', () => {});

    expect(subscribeCalls(transport.sent, 'bridge.subscribe', 'mpris.event')).toBe(2);
    expect(subscribeCalls(transport.sent, 'bridge.unsubscribe', 'mpris.event')).toBe(1);
  });
});
