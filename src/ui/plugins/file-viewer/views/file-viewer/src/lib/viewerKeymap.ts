// Pure keyboard-shortcut resolver for the file viewer, mirroring the
// explorer's keymap.ts shape: a (event) => action descriptor function with no
// DOM or state access. The caller (App.svelte) decides what each action means
// given current UI state (for example, whether the search bar is already
// open) — this module only reports which shortcut was pressed.

export type ViewerShortcutAction =
    | { kind: 'open-search' }
    | { kind: 'next-match' }
    | { kind: 'previous-match' }
    | { kind: 'close-search' }
    // Diff-mode-only shortcuts (qv diff mode design doc, "Keyboard"). Reported
    // unconditionally by this resolver regardless of which mode is active —
    // the caller (App.svelte for file mode, DiffView for diff mode) decides
    // whether the action means anything in its current UI state. None of
    // these fire with Ctrl, Meta, or Alt held.
    | { kind: 'next-change' }
    | { kind: 'previous-change' }
    | { kind: 'next-file' }
    | { kind: 'previous-file' }
    | { kind: 'toggle-stage' }
    | { kind: 'refresh' };

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

    // None of the diff-only shortcuts below take Ctrl/Meta — a held control
    // modifier always falls through to null from here, leaving those
    // combinations free for the browser/compositor.
    if (control) {
        return null;
    }

    switch (event.key) {
        case 'n':
            return { kind: 'next-change' };
        case 'p':
            return { kind: 'previous-change' };
        case ']':
            return { kind: 'next-file' };
        case '[':
            return { kind: 'previous-file' };
        case 's':
            return { kind: 'toggle-stage' };
        case 'r':
        case 'R':
            return { kind: 'refresh' };
        default:
            return null;
    }
}
