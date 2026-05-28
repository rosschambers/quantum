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
    (globalThis as any).window.__quantum_resolve(7, JSON.stringify({ ok: true }));
    expect(received).toHaveLength(1);
    expect(received[0]).toEqual({ jsonrpc: '2.0', id: 7, result: { ok: true } });
  });
});
