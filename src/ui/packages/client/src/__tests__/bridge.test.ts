import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createBridgeTransport } from '../bridge';

describe('bridge transport', () => {
  beforeEach(() => {
    (globalThis as any).window = {
      webkit: {
        messageHandlers: {
          quantum: { postMessage: vi.fn() },
        },
      },
    };
  });

  afterEach(() => {
    delete (globalThis as any).window;
  });

  it('sends via window.webkit.messageHandlers.quantum.postMessage', () => {
    const transport = createBridgeTransport();
    expect(transport).not.toBeNull();
    if (!transport) return;

    transport.send({ jsonrpc: '2.0', id: 1, method: 'test', params: {} });
    const post = (globalThis as any).window.webkit.messageHandlers.quantum.postMessage;
    expect(post).toHaveBeenCalledTimes(1);
    const arg = post.mock.calls[0][0];
    const parsed = JSON.parse(arg);
    expect(parsed.method).toBe('test');
  });

  it('dispatches resolve callbacks via window.__quantum_resolve', () => {
    const transport = createBridgeTransport();
    expect(transport).not.toBeNull();
    if (!transport) return;

    const received: any[] = [];
    transport.onResponse((m) => received.push(m));
    (globalThis as any).window.__quantum_resolve(7, { ok: true });
    expect(received).toHaveLength(1);
    expect(received[0]).toEqual({ jsonrpc: '2.0', id: 7, result: { ok: true } });
  });

  it('dispatches reject callbacks via window.__quantum_reject with structured error', () => {
    const transport = createBridgeTransport();
    expect(transport).not.toBeNull();
    if (!transport) return;

    const received: any[] = [];
    transport.onResponse((m) => received.push(m));
    (globalThis as any).window.__quantum_reject(9, { code: -32000, message: 'boom' });
    expect(received).toHaveLength(1);
    expect(received[0]).toEqual({
      jsonrpc: '2.0',
      id: 9,
      error: { code: -32000, message: 'boom', data: undefined },
    });
  });

  it('routes each reply to the transport that sent the request when a page has several clients', () => {
    // Two clients on one page (App plus a child component) both number their
    // requests from 1. The single window-level reply handler must deliver each
    // reply to its own sender, under that sender's original id.
    const first = createBridgeTransport();
    const second = createBridgeTransport();
    if (!first || !second) throw new Error('transport unavailable');

    const firstReceived: any[] = [];
    const secondReceived: any[] = [];
    first.onResponse((m) => firstReceived.push(m));
    second.onResponse((m) => secondReceived.push(m));

    first.send({ jsonrpc: '2.0', id: 1, method: 'first.method', params: {} });
    second.send({ jsonrpc: '2.0', id: 1, method: 'second.method', params: {} });

    const post = (globalThis as any).window.webkit.messageHandlers.quantum.postMessage;
    const [firstWire, secondWire] = post.mock.calls.map((call: unknown[]) => JSON.parse(call[0] as string));
    expect(firstWire.id).not.toBe(secondWire.id);

    (globalThis as any).window.__quantum_resolve(secondWire.id, { from: 'second' });
    (globalThis as any).window.__quantum_reject(firstWire.id, { code: -32000, message: 'first failed' });

    expect(secondReceived).toEqual([{ jsonrpc: '2.0', id: 1, result: { from: 'second' } }]);
    expect(firstReceived).toEqual([
      { jsonrpc: '2.0', id: 1, error: { code: -32000, message: 'first failed', data: undefined } },
    ]);
  });

  it('delivers notifications to every transport on the page', () => {
    const first = createBridgeTransport();
    const second = createBridgeTransport();
    if (!first || !second) throw new Error('transport unavailable');

    const firstReceived: any[] = [];
    const secondReceived: any[] = [];
    first.onNotification((n) => firstReceived.push(n));
    second.onNotification((n) => secondReceived.push(n));

    (globalThis as any).window.__quantum_notify('files.event', { event: 'changed' });

    expect(firstReceived).toEqual([{ channel: 'files.event', payload: { event: 'changed' } }]);
    expect(secondReceived).toEqual([{ channel: 'files.event', payload: { event: 'changed' } }]);
  });

  it('preserves U+2028 in resolved payload (no JSON.parse round-trip)', () => {
    const transport = createBridgeTransport();
    expect(transport).not.toBeNull();
    if (!transport) return;

    const received: any[] = [];
    transport.onResponse((m) => received.push(m));
    (globalThis as any).window.__quantum_resolve(11, { title: 'line\u2028break' });
    expect(received).toHaveLength(1);
    expect(received[0].result).toEqual({ title: 'line\u2028break' });
  });
});
