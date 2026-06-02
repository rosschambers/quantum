export interface JsonRpcRequest {
  jsonrpc: '2.0';
  id: number;
  method: string;
  params?: unknown;
}

export interface JsonRpcResponse {
  jsonrpc: '2.0';
  id: number;
  result?: unknown;
  error?: {
    code: number;
    message: string;
    data?: unknown;
  };
}

export interface JsonRpcNotification {
  channel: string;
  payload: unknown;
}

export interface Transport {
  send(request: JsonRpcRequest): void;
  onResponse(callback: (response: JsonRpcResponse) => void): () => void;
  onNotification(callback: (notification: JsonRpcNotification) => void): () => void;
}

/**
 * Create an in-memory bidirectional transport pair for testing.
 * Returns a transport that can respond via `respondWith()` and `notify()`.
 */
export function createMockTransport(): Transport & {
  respondWith(response: JsonRpcResponse): void;
  notify(notification: JsonRpcNotification): void;
} {
  const responseCallbacks: ((response: JsonRpcResponse) => void)[] = [];
  const notificationCallbacks: ((notification: JsonRpcNotification) => void)[] = [];

  return {
    send(request: JsonRpcRequest): void {
      // In a real transport, this would send the request somewhere.
      // For mocking, we just store it and allow tests to respond.
    },

    onResponse(callback: (response: JsonRpcResponse) => void): () => void {
      responseCallbacks.push(callback);
      return () => {
        const idx = responseCallbacks.indexOf(callback);
        if (idx !== -1) responseCallbacks.splice(idx, 1);
      };
    },

    onNotification(callback: (notification: JsonRpcNotification) => void): () => void {
      notificationCallbacks.push(callback);
      return () => {
        const idx = notificationCallbacks.indexOf(callback);
        if (idx !== -1) notificationCallbacks.splice(idx, 1);
      };
    },

    respondWith(response: JsonRpcResponse): void {
      // Simulate async response delivery
      responseCallbacks.forEach((cb) => cb(response));
    },

    notify(notification: JsonRpcNotification): void {
      // Simulate async notification delivery
      notificationCallbacks.forEach((cb) => cb(notification));
    },
  };
}
