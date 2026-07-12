// Pure keyboard-shortcut resolver for the file explorer. Maps a keyboard event
// to an action descriptor without touching the DOM, mirroring how menus.ts maps
// a menu selection to a callback. App.svelte's onKeyDown calls this and
// dispatches the returned action to the existing handlers. Only the "management"
// shortcuts live here; arrow/Enter/Tab/Ctrl+L/type-ahead stay in onKeyDown.

/** An action a keyboard shortcut requests the App to perform. */
export type ShortcutAction =
    | { kind: 'select-all' }
    | { kind: 'clipboard'; operation: 'copy' | 'cut' }
    | { kind: 'paste' }
    | { kind: 'trash' }
    | { kind: 'delete-permanent' }
    | { kind: 'rename' }
    | { kind: 'duplicate' }
    | { kind: 'new-folder' }
    | { kind: 'refresh' }
    | { kind: 'cursor'; to: 'first' | 'last' }
    | { kind: 'toggle-hidden' }
    | { kind: 'clear-selection' };

/** Map a keyboard event to a shortcut action, or null when it is not one. */
export function resolveShortcut(event: KeyboardEvent): ShortcutAction | null {
    const control = event.ctrlKey || event.metaKey;
    // Alt is reserved for navigation (Alt+arrows); never a management shortcut.
    if (event.altKey) {
        return null;
    }
    const lower = event.key.toLowerCase();

    if (control && !event.shiftKey) {
        switch (lower) {
            case 'a': return { kind: 'select-all' };
            case 'c': return { kind: 'clipboard', operation: 'copy' };
            case 'x': return { kind: 'clipboard', operation: 'cut' };
            case 'v': return { kind: 'paste' };
            case 'd': return { kind: 'duplicate' };
            case 'h': return { kind: 'toggle-hidden' };
            default: return null;
        }
    }
    if (control && event.shiftKey && lower === 'n') {
        return { kind: 'new-folder' };
    }
    if (control) {
        return null;
    }

    switch (event.key) {
        case 'Delete': return event.shiftKey ? { kind: 'delete-permanent' } : { kind: 'trash' };
        case 'F2': return { kind: 'rename' };
        case 'F5': return { kind: 'refresh' };
        case 'Home': return { kind: 'cursor', to: 'first' };
        case 'End': return { kind: 'cursor', to: 'last' };
        case 'Escape': return { kind: 'clear-selection' };
        default: return null;
    }
}
