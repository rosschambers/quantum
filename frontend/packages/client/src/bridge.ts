import type { Transport, JsonRpcRequest, JsonRpcResponse, JsonRpcNotification } from './transport';

declare global {
  interface Window {
    webkit?: {
      messageHandlers?: {
        quantum?: {
          postMessage(message: unknown): void;
        };
      };
    };
    __quantum_resolve?: (id: number, result: unknown) => void;
    __quantum_reject?: (id: number, error: unknown) => void;
    __quantum_notify?: (channel: string, payload: unknown) => void;
  }
}

export function createBridgeTransport(): Transport | null {
  if (typeof window === 'undefined' || !window.webkit?.messageHandlers?.quantum) {
    return null;
  }

  const responseCallbacks: ((response: JsonRpcResponse) => void)[] = [];
  const notificationCallbacks: ((notification: JsonRpcNotification) => void)[] = [];

  // Install global handlers for receiving responses and notifications
  if (!window.__quantum_resolve) {
    window.__quantum_resolve = (id: number, result: unknown) => {
      const parsedResult = typeof result === 'string' ? JSON.parse(result) : result;
      const response: JsonRpcResponse = {
        jsonrpc: '2.0',
        id,
        result: parsedResult,
      };
      responseCallbacks.forEach((cb) => cb(response));
    };
  }

  if (!window.__quantum_reject) {
    window.__quantum_reject = (id: number, error: unknown) => {
      const errorObj = error as any || {};
      const response: JsonRpcResponse = {
        jsonrpc: '2.0',
        id,
        error: {
          code: errorObj.code ?? -32603,
          message: errorObj.message ?? 'Internal error',
          data: errorObj.data,
        },
      };
      responseCallbacks.forEach((cb) => cb(response));
    };
  }

  if (!window.__quantum_notify) {
    window.__quantum_notify = (channel: string, payload: unknown) => {
      const notification: JsonRpcNotification = { channel, payload };
      notificationCallbacks.forEach((cb) => cb(notification));
    };
  }

  return {
    send(request: JsonRpcRequest): void {
      window.webkit!.messageHandlers!.quantum!.postMessage(JSON.stringify(request));
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
  };
}
