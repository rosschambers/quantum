// A tiny reactive toast store for the file-explorer view. Operations show a
// transient confirmation (or an error) in the bottom-right corner; each toast
// auto-dismisses after a fixed duration. Written as a Svelte 5 runes module so
// `Toasts.svelte` renders the exported `toasts` array reactively. The array is
// mutated in place (push/splice) so the shared proxy reference stays stable
// across the module boundary.

/** Whether a toast is a neutral confirmation or an error. */
export type ToastKind = 'info' | 'error';

/** A single toast: a stable id, its message, and its kind. */
export interface Toast {
    id: number;
    message: string;
    kind: ToastKind;
}

/** How long a toast stays on screen before it auto-dismisses, in milliseconds. */
export const TOAST_DURATION_MILLISECONDS = 2600;

/** The live list of visible toasts, newest last. */
export const toasts = $state<Toast[]>([]);

let nextId = 1;

/**
 * Show a toast. Defaults to the neutral `info` kind. Returns the new toast's id
 * so a caller can dismiss it early via `dismissToast`. The toast auto-dismisses
 * after `TOAST_DURATION_MILLISECONDS`.
 */
export function pushToast(message: string, kind: ToastKind = 'info'): number {
    const id = nextId;
    nextId += 1;
    toasts.push({ id, message, kind });
    setTimeout(() => dismissToast(id), TOAST_DURATION_MILLISECONDS);
    return id;
}

/** Remove the toast with the given id, if it is still present. */
export function dismissToast(id: number): void {
    const index = toasts.findIndex((toast) => toast.id === id);
    if (index !== -1) {
        toasts.splice(index, 1);
    }
}
