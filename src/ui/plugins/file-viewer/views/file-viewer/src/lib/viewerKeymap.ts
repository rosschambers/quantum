// Pure keyboard-shortcut resolver for the file viewer, mirroring the
// explorer's keymap.ts shape: a (event) => action descriptor function with no
// DOM or state access. The caller (App.svelte) decides what each action means
// given current UI state (for example, whether the search bar is already
// open) — this module only reports which shortcut was pressed.

export type ViewerShortcutAction =
    | { kind: 'open-search' }
    | { kind: 'next-match' }
    | { kind: 'previous-match' }
    | { kind: 'close-search' };

/** Map a keyboard event to a viewer shortcut action, or null when it is not one. */
export function resolveViewerShortcut(event: KeyboardEvent): ViewerShortcutAction | null {
    // Alt is reserved for navigation (mirrors the explorer's convention).
    if (event.altKey) {
        return null;
    }

    const control = event.ctrlKey || event.metaKey;

    if (control && !event.shiftKey && event.key.toLowerCase() === 'f') {
        return { kind: 'open-search' };
    }

    if (event.key === 'Enter') {
        return event.shiftKey ? { kind: 'previous-match' } : { kind: 'next-match' };
    }

    if (event.key === 'Escape') {
        return { kind: 'close-search' };
    }

    return null;
}
