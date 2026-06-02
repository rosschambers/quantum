import { describe, it, expect, beforeEach } from 'vitest';
import { createClient } from '../index';
import { createMockTransport } from '../transport';

describe('@quantum/client', () => {
  describe('call', () => {
    it('resolves with matching response payload', async () => {
      const transport = createMockTransport();
      const client = createClient({ transport });

      const callPromise = client.call('system.status', {});

      // Allow the promise to be created and pending call to be registered
      await Promise.resolve();

      transport.respondWith({
        jsonrpc: '2.0',
        id: 1,
        result: { version: '0.1.0' },
      });

      const result = await callPromise;
      expect(result).toEqual({ version: '0.1.0' });
    });

    it('rejects with error object when transport returns a JSON-RPC error', async () => {
      const transport = createMockTransport();
      const client = createClient({ transport });

      const callPromise = client.call('some.method', {});

      await Promise.resolve();

      transport.respondWith({
        jsonrpc: '2.0',
        id: 1,
        error: {
          code: -32001,
          message: 'provider not found',
          data: { provider: 'apps' },
        },
      });

      await expect(callPromise).rejects.toMatchObject({
        code: -32001,
        message: 'provider not found',
        data: { provider: 'apps' },
      });
    });

    it('resolves multiple in-flight calls in arrival order regardless of dispatch order', async () => {
      const transport = createMockTransport();
      const client = createClient({ transport });

      const call1 = client.call('search', { text: 'a' });
      const call2 = client.call('search', { text: 'b' });
      const call3 = client.call('search', { text: 'c' });

      await Promise.resolve();

      // Respond in non-sequential order
      transport.respondWith({
        jsonrpc: '2.0',
        id: 2,
        result: { matches: ['result2'] },
      });

      transport.respondWith({
        jsonrpc: '2.0',
        id: 1,
        result: { matches: ['result1'] },
      });

      transport.respondWith({
        jsonrpc: '2.0',
        id: 3,
        result: { matches: ['result3'] },
      });

      const [res1, res2, res3] = await Promise.all([call1, call2, call3]);

      expect(res1).toEqual({ matches: ['result1'] });
      expect(res2).toEqual({ matches: ['result2'] });
      expect(res3).toEqual({ matches: ['result3'] });
    });
  });

  describe('subscribe', () => {
    it('fires callback exactly when a notification arrives for that channel', async () => {
      const transport = createMockTransport();
      const client = createClient({ transport });

      const results: unknown[] = [];
      client.subscribe('theme-changed', (payload) => {
        results.push(payload);
      });

      transport.notify({
        channel: 'theme-changed',
        payload: { name: 'dark' },
      });

      // Give async handler a moment to process
      await new Promise((resolve) => setTimeout(resolve, 10));

      expect(results).toHaveLength(1);
      expect(results[0]).toEqual({ name: 'dark' });
    });

    it('ignores notifications for other channels', async () => {
      const transport = createMockTransport();
      const client = createClient({ transport });

      const results: unknown[] = [];
      client.subscribe('theme-changed', (payload) => {
        results.push(payload);
      });

      transport.notify({
        channel: 'other-channel',
        payload: { data: 'ignored' },
      });

      transport.notify({
        channel: 'theme-changed',
        payload: { name: 'dark' },
      });

      await new Promise((resolve) => setTimeout(resolve, 10));

      expect(results).toHaveLength(1);
      expect(results[0]).toEqual({ name: 'dark' });
    });

    it('unsubscribe stops further callbacks', async () => {
      const transport = createMockTransport();
      const client = createClient({ transport });

      const results: unknown[] = [];
      const unsubscribe = client.subscribe('theme-changed', (payload) => {
        results.push(payload);
      });

      transport.notify({
        channel: 'theme-changed',
        payload: { name: 'dark' },
      });

      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(results).toHaveLength(1);

      unsubscribe();

      transport.notify({
        channel: 'theme-changed',
        payload: { name: 'light' },
      });

      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(results).toHaveLength(1); // still just 1
    });
  });
});
