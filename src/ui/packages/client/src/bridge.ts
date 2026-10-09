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
    __quantum_reject?: (id: number, error: { code?: number; message?: string; data?: unknown }) => void;
    __quantum_notify?: (channel: string, payload: unknown) => void;
  }
}

interface PendingRoute {
  originalId: number;
  deliver: (response: JsonRpcResponse) => void;
}

/** Bridge state shared by every transport on one page. */
interface PageRegistry {
  nextWireId: number;
  pending: Map<number, PendingRoute>;
  notificationListeners: Set<((notification: JsonRpcNotification) => void)[]>;
  /** Fallback for replies to ids this page never routed (legacy callers). */
  unrouted: Set<(response: JsonRpcResponse) => void>;
}

/**
 * The page-wide registry, created on first use together with the window-level
 * handlers the host calls. The Rust side splices JSON directly into a JS
 * expression, so the handlers receive structured values — no JSON.parse needed.
 * Kept on `window` so each page (and each test's fake window) has its own.
 */
function pageRegistry(): PageRegistry {
  const host = window as Window & { __quantum_bridge_registry?: PageRegistry };
  if (host.__quantum_bridge_registry) {
    return host.__quantum_bridge_registry;
  }
  const registry: PageRegistry = {
    nextWireId: 0,
    pending: new Map(),
    notificationListeners: new Set(),
    unrouted: new Set(),
  };
  host.__quantum_bridge_registry = registry;

  function route(id: number, build: (originalId: number) => JsonRpcResponse): void {
    const target = registry.pending.get(id);
    if (target) {
      registry.pending.delete(id);
      target.deliver(build(target.originalId));
      return;
    }
    const response = build(id);
    registry.unrouted.forEach((deliver) => deliver(response));
  }

  window.__quantum_resolve = (id: number, result: unknown) => {
    route(id, (originalId) => ({ jsonrpc: '2.0', id: originalId, result }));
  };

  window.__quantum_reject = (id: number, error: { code?: number; message?: string; data?: unknown }) => {
    const errorObj = error ?? {};
    route(id, (originalId) => ({
      jsonrpc: '2.0',
      id: originalId,
      error: {
        code: errorObj.code ?? -32603,
        message: errorObj.message ?? 'Internal error',
        data: errorObj.data,
      },
    }));
  };

  window.__quantum_notify = (channel: string, payload: unknown) => {
    const notification: JsonRpcNotification = { channel, payload };
    registry.notificationListeners.forEach((callbacks) => callbacks.forEach((cb) => cb(notification)));
  };

  return registry;
}

export function createBridgeTransport(): Transport | null {
  if (typeof window === 'undefined' || !window.webkit?.messageHandlers?.quantum) {
    return null;
  }

  const responseCallbacks: ((response: JsonRpcResponse) => void)[] = [];
  const notificationCallbacks: ((notification: JsonRpcNotification) => void)[] = [];
  const registry = pageRegistry();
  registry.notificationListeners.add(notificationCallbacks);

  function deliver(response: JsonRpcResponse): void {
    responseCallbacks.forEach((cb) => cb(response));
  }
  registry.unrouted.add(deliver);

  return {
    send(request: JsonRpcRequest): void {
      // Several clients can live on one page (a view plus its child
      // components), each numbering requests from 1, while the host echoes the
      // id back to ONE window-level handler. Rewrite the id to a page-unique
      // wire id and remember who sent it, so the reply reaches its sender
      // under the sender's own id.
      const wireId = ++registry.nextWireId;
      registry.pending.set(wireId, { originalId: request.id, deliver });
      window.webkit!.messageHandlers!.quantum!.postMessage(JSON.stringify({ ...request, id: wireId }));
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
