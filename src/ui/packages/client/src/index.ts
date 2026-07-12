import type { Transport, JsonRpcRequest, JsonRpcResponse, JsonRpcNotification } from './transport';
import { createMockTransport } from './transport';
import { createBridgeTransport } from './bridge';

export type { Transport, JsonRpcRequest, JsonRpcResponse, JsonRpcNotification };

export interface ClientError {
  code: number;
  message: string;
  data?: unknown;
}

export interface Client {
  call(method: string, params: unknown): Promise<unknown>;
  subscribe(channel: string, callback: (payload: unknown) => void): () => void;
  close(): void;
}

interface PendingCall {
  resolve: (value: unknown) => void;
  reject: (reason: ClientError) => void;
}

export function createClient(options?: { transport?: Transport }): Client {
  const transport = options?.transport ?? 
    (() => {
      const w = typeof window !== 'undefined' ? (window as any) : undefined;
      return w?.webkit?.messageHandlers?.quantum 
        ? createBridgeTransport() 
        : createMockTransport();
    })();

  if (!transport) {
    throw new Error('Failed to initialize transport');
  }

  let nextId = 1;
  const pending = new Map<number, PendingCall>();
  const subscriptions = new Map<string, Set<(payload: unknown) => void>>();

  // Listen for responses
  const unsubscribeResponse = transport.onResponse((response: JsonRpcResponse) => {
    const pending_call = pending.get(response.id);
    if (!pending_call) {
      console.warn(`Received response for unknown request id: ${response.id}`);
      return;
    }

    pending.delete(response.id);

    if (response.error) {
      pending_call.reject({
        code: response.error.code,
        message: response.error.message,
        data: response.error.data,
      });
    } else {
      pending_call.resolve(response.result);
    }
  });

  // Listen for notifications
  const unsubscribeNotification = transport.onNotification((notification: JsonRpcNotification) => {
    const callbacks = subscriptions.get(notification.channel);
    if (callbacks) {
      callbacks.forEach((cb) => cb(notification.payload));
    }
  });

  return {
    call(method: string, params: unknown): Promise<unknown> {
      const id = nextId++;
      const request: JsonRpcRequest = {
        jsonrpc: '2.0',
        id,
        method,
        params,
      };

      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        transport.send(request);
      });
    },

    subscribe(channel: string, callback: (payload: unknown) => void): () => void {
      if (!subscriptions.has(channel)) {
        subscriptions.set(channel, new Set());
      }

      const callbacks = subscriptions.get(channel)!;
      callbacks.add(callback);

      return () => {
        callbacks.delete(callback);
        if (callbacks.size === 0) {
          subscriptions.delete(channel);
        }
      };
    },

    close(): void {
      pending.clear();
      subscriptions.clear();
      unsubscribeResponse();
      unsubscribeNotification();
    },
  };
}

export {
  openContextMenu,
  closeContextMenu,
  clampToViewport,
  type MenuItem,
  type MenuOptions,
} from './contextMenu';
export {
  createNotificationStore,
  type PendingNotification,
  type NotificationChange,
  type NotificationEnvelope,
  type NotificationStore,
} from './notifications';
export {
  createTimerStore,
  type VisualStyle,
  type FillBorderColor,
  type TextVisibility,
  type TextPosition,
  type TextColor,
  type TimeFormat,
  type SoundName,
  type Weekday,
  type TimeOfDay,
  type VisualConfig,
  type NotifyConfig,
  type TimerKind,
  type Point,
  type TimerStatus,
  type Timer,
  type TimerSettings,
  type TimerStoreData,
  type TimerEnvelope,
  type TimerStore,
} from './timer';
export {
  type FileEntryKind,
  type PermissionClass,
  type ContentKind,
  type FileEntry,
  type DriveInfo,
  type Pin,
  type ApplicationInfo,
  type FileOperation,
  type Places,
  type FilePreferences,
  type PreviewKind,
  type PreviewPayload,
  type FilesEvent,
} from './files';
export {
  PROCESSES_EVENT_CHANNEL,
  PROCESSES_WATCH,
  PROCESSES_UNWATCH,
  PROCESSES_KILL,
  type GlobalStats,
  type WindowInfo,
  type ProcessNode,
  type ProcessSnapshot,
  type KillSignal,
} from './processes';
export {
  type IconRef,
  type SystemTrayMenuNode,
  type SystemTrayItem,
  type SystemTrayState,
} from './systemTray';
export { type ShellCaptureResult } from './shellCapture';
