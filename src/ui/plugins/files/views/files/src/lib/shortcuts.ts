// The single source of truth for the file explorer's keyboard shortcuts. Both
// surfaces that show shortcuts read from here so they can never drift apart:
// `menus.ts` pulls the accelerator labels for context-menu items from
// `SHORTCUT_KEYS`, and `ShortcutsModal.svelte` renders `SHORTCUT_GROUPS` as the
// grouped cheat sheet. The key labels are display strings only; the actual
// keyboard handling lives in `keymap.ts` and the App's key handler.

/**
 * A stable action name mapped to its human-readable key label. Referenced by
 * both the context-menu accelerators and the cheat sheet so the two agree.
 */
export const SHORTCUT_KEYS = {
    selectAll: 'Ctrl+A',
    clearSelection: 'Esc',
    copy: 'Ctrl+C',
    cut: 'Ctrl+X',
    paste: 'Ctrl+V',
    trash: 'Del',
    deletePermanent: 'Shift+Del',
    rename: 'F2',
    duplicate: 'Ctrl+D',
    newFolder: 'Ctrl+Shift+N',
    open: 'Enter',
    parent: 'Backspace',
    back: 'Alt+Left',
    forward: 'Alt+Right',
    firstLast: 'Home / End',
    switchPane: 'Tab',
    toggleHidden: 'Ctrl+H',
    refresh: 'F5',
    locationBar: 'Ctrl+L',
    help: '?',
} as const;

/** A single shortcut row: its key label and what it does. */
export interface ShortcutHint {
    keys: string;
    description: string;
}

/** A titled group of shortcut rows, as shown in the cheat sheet. */
export interface ShortcutGroup {
    title: string;
    hints: ShortcutHint[];
}

/**
 * The cheat sheet, grouped by section. Every hint's `keys` references a
 * `SHORTCUT_KEYS` value rather than repeating a literal, so the accelerator
 * labels and the cheat sheet stay in lockstep.
 */
export const SHORTCUT_GROUPS: ShortcutGroup[] = [
    {
        title: 'Selection',
        hints: [
            { keys: SHORTCUT_KEYS.selectAll, description: 'Select all' },
            { keys: SHORTCUT_KEYS.clearSelection, description: 'Clear selection' },
        ],
    },
    {
        title: 'Clipboard',
        hints: [
            { keys: SHORTCUT_KEYS.copy, description: 'Copy' },
            { keys: SHORTCUT_KEYS.cut, description: 'Cut' },
            { keys: SHORTCUT_KEYS.paste, description: 'Paste' },
        ],
    },
    {
        title: 'File',
        hints: [
            { keys: SHORTCUT_KEYS.trash, description: 'Move to trash' },
            { keys: SHORTCUT_KEYS.deletePermanent, description: 'Delete permanently' },
            { keys: SHORTCUT_KEYS.rename, description: 'Rename' },
            { keys: SHORTCUT_KEYS.duplicate, description: 'Duplicate' },
            { keys: SHORTCUT_KEYS.newFolder, description: 'New folder' },
        ],
    },
    {
        title: 'Navigation',
        hints: [
            { keys: SHORTCUT_KEYS.open, description: 'Open' },
            { keys: SHORTCUT_KEYS.parent, description: 'Parent folder' },
            { keys: SHORTCUT_KEYS.back, description: 'Back' },
            { keys: SHORTCUT_KEYS.forward, description: 'Forward' },
            { keys: SHORTCUT_KEYS.firstLast, description: 'First / last' },
            { keys: SHORTCUT_KEYS.switchPane, description: 'Switch pane' },
        ],
    },
    {
        title: 'View',
        hints: [
            { keys: SHORTCUT_KEYS.toggleHidden, description: 'Toggle hidden files' },
            { keys: SHORTCUT_KEYS.refresh, description: 'Refresh' },
            { keys: SHORTCUT_KEYS.locationBar, description: 'Location bar' },
            { keys: SHORTCUT_KEYS.help, description: 'Keyboard shortcuts' },
        ],
    },
];
