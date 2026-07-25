// Pure builders for the file-explorer's two right-click menus: the entry menu
// (right-click a row) and the background menu (right-click empty list space).
// Each takes a context of the target, the current selection, and action
// callbacks, and returns a `MenuItem[]` for `openContextMenu` from
// `@quantum/client`. The builders never touch the DOM or perform IPC — they map
// a menu selection to a callback — so they are trivially unit-testable and the
// App owns all side effects (IPC, toasts, inline rename, modal properties).
//
// Note on "Open with": `MenuItem` has no nested-submenu field (it is
// `{ label, icon?, disabled?, danger?, separator?, onSelect }`), so a real
// applications submenu is not possible in one menu. The entry menu therefore
// offers a single "Open with..." item that calls `onOpenWithPicker`; the App
// responds by opening a second `openContextMenu` populated from
// `files.applications`.

import type { ApplicationInfo, FileEntry, FileOperation, MenuItem, PinnedAction } from '@quantum/client';
import type { ClipboardOperation } from './clipboard.svelte';
import { pathBaseName } from './path';
import { SHORTCUT_KEYS } from './shortcuts';

/** A pin target: the path to pin and the label to show for it. A `FileEntry` satisfies this. */
export interface PinTarget {
    path: string;
    name: string;
}

/** Everything the entry menu needs: the target, the selection, and action callbacks. */
export interface EntryMenuContext {
    /** The row that was right-clicked. */
    entry: FileEntry;
    /** The paths currently selected in the pane, for multi-select operations. */
    selectionPaths: string[];
    /** The directory currently shown in the pane. */
    path: string;
    /** Whether a given path is already pinned in the sidebar. */
    isPinned: (path: string) => boolean;
    /** Applications that can open files; the App uses these to fill the Open-with picker. */
    applications: ApplicationInfo[];
    /** Open the entry with its default application. */
    onOpen: (entry: FileEntry) => void;
    /** Ask the App to open a secondary applications picker menu for the entry. */
    onOpenWithPicker: (entry: FileEntry) => void;
    /** Open a terminal in the given directory. */
    onOpenTerminal: (directory: string) => void;
    /** Record a cut or copy of the given paths in the in-view clipboard. */
    onClipboard: (operation: ClipboardOperation, paths: string[]) => void;
    /** Paste the in-view clipboard into the given directory. */
    onPaste: (directory: string) => void;
    /** Dispatch a file-system operation. */
    onOperation: (op: FileOperation) => void;
    /** Begin an inline rename of the entry. */
    onRename: (entry: FileEntry) => void;
    /** Pin a directory to the sidebar. */
    onPin: (target: PinTarget) => void;
    /** Unpin a path from the sidebar. */
    onUnpin: (path: string) => void;
    /** Write a path to the system clipboard via `navigator.clipboard`. */
    onCopyPath: (path: string) => void;
    /** Open the properties modal for the entry. */
    onProperties: (entry: FileEntry) => void;
    /** User-pinned "open with" actions, prepended above Open when non-empty. */
    pinnedActions: PinnedAction[];
    /** Open a path with a pinned application, identified by its desktop id. */
    onOpenWithPinned: (desktopId: string, path: string) => void;
}

/** Everything the background (empty-space) menu needs. */
export interface BackgroundMenuContext {
    /** The directory currently shown in the pane. */
    path: string;
    /** Prompt for and create a new folder in the directory. */
    onNewFolder: (directory: string) => void;
    /** Prompt for and create a new file in the directory. */
    onNewFile: (directory: string) => void;
    /** Paste the in-view clipboard into the directory. */
    onPaste: (directory: string) => void;
    /** Open a terminal in the directory. */
    onOpenTerminal: (directory: string) => void;
    /** Pin the directory to the sidebar. */
    onPin: (target: PinTarget) => void;
    /** Open the properties modal for the directory. */
    onProperties: (target: PinTarget) => void;
    /** User-pinned "open with" actions, prepended above New folder when non-empty. */
    pinnedActions: PinnedAction[];
    /** Open a path with a pinned application, identified by its desktop id. */
    onOpenWithPinned: (desktopId: string, path: string) => void;
}

/** Join a directory and a leaf name into an absolute path without a double slash at the root. */
function joinPath(directory: string, name: string): string {
    return directory === '/' ? `/${name}` : `${directory}/${name}`;
}

/** A separator menu item. */
const separator: MenuItem = { separator: true };

/** Build the leading pinned "open with" items for a target path, or [] when none. */
function pinnedItems(
    pinnedActions: PinnedAction[],
    targetPath: string,
    onOpenWithPinned: (desktopId: string, path: string) => void,
): MenuItem[] {
    if (pinnedActions.length === 0) {
        return [];
    }
    const items: MenuItem[] = pinnedActions.map((action) => ({
        label: action.label,
        onSelect: () => onOpenWithPinned(action.desktop_id, targetPath),
    }));
    items.push(separator);
    return items;
}

/**
 * Build the entry (row) context menu. The operation target is the whole
 * selection when the right-clicked entry is part of a multi-selection,
 * otherwise just the entry itself.
 */
export function buildEntryMenu(ctx: EntryMenuContext): MenuItem[] {
    const { entry } = ctx;
    const paths =
        ctx.selectionPaths.includes(entry.path) && ctx.selectionPaths.length > 1
            ? ctx.selectionPaths
            : [entry.path];
    const terminalDirectory = entry.kind === 'directory' ? entry.path : ctx.path;

    const items: MenuItem[] = [
        ...pinnedItems(ctx.pinnedActions, entry.path, ctx.onOpenWithPinned),
        { label: 'Open', onSelect: () => ctx.onOpen(entry) },
        { label: 'Open with...', onSelect: () => ctx.onOpenWithPicker(entry) },
        { label: 'Open terminal here', onSelect: () => ctx.onOpenTerminal(terminalDirectory) },
        separator,
        { label: 'Cut', shortcut: SHORTCUT_KEYS.cut, onSelect: () => ctx.onClipboard('cut', paths) },
        { label: 'Copy', shortcut: SHORTCUT_KEYS.copy, onSelect: () => ctx.onClipboard('copy', paths) },
        { label: 'Paste', shortcut: SHORTCUT_KEYS.paste, onSelect: () => ctx.onPaste(ctx.path) },
        {
            label: 'Duplicate',
            shortcut: SHORTCUT_KEYS.duplicate,
            onSelect: () => ctx.onOperation({ kind: 'duplicate', path: entry.path }),
        },
        { label: 'Rename', shortcut: SHORTCUT_KEYS.rename, onSelect: () => ctx.onRename(entry) },
        separator,
    ];

    if (entry.content_kind === 'archive') {
        items.push({ label: 'Extract', onSelect: () => ctx.onOperation({ kind: 'extract', path: entry.path }) });
    } else {
        // The menu builder is synchronous and has no view of the directory's
        // contents (the pane may be showing a filtered or deep-search listing,
        // not the true directory), so it cannot guarantee a collision-free name.
        // It emits a candidate "<name>.tar.zst"; making the archive name
        // collision-safe (for example appending " (2)") is the backend's
        // responsibility when it performs the compress operation.
        const destination = joinPath(ctx.path, `${entry.name}.tar.zst`);
        items.push({
            label: 'Compress',
            onSelect: () => ctx.onOperation({ kind: 'compress', paths, destination }),
        });
    }

    items.push(separator);

    if (entry.kind === 'directory') {
        if (ctx.isPinned(entry.path)) {
            items.push({ label: 'Unpin from sidebar', onSelect: () => ctx.onUnpin(entry.path) });
        } else {
            items.push({ label: 'Pin to sidebar', onSelect: () => ctx.onPin(entry) });
        }
    }
    items.push({ label: 'Copy path', onSelect: () => ctx.onCopyPath(entry.path) });

    items.push(
        separator,
        {
            label: 'Move to trash',
            shortcut: SHORTCUT_KEYS.trash,
            danger: true,
            onSelect: () => ctx.onOperation({ kind: 'trash', paths }),
        },
        {
            label: 'Delete permanently',
            shortcut: SHORTCUT_KEYS.deletePermanent,
            danger: true,
            onSelect: () => ctx.onOperation({ kind: 'delete', paths }),
        },
        separator,
        { label: 'Properties', onSelect: () => ctx.onProperties(entry) },
    );

    return items;
}

/** Build the background (empty list space) context menu for the current directory. */
export function buildBackgroundMenu(ctx: BackgroundMenuContext): MenuItem[] {
    const target: PinTarget = { path: ctx.path, name: pathBaseName(ctx.path) };
    return [
        ...pinnedItems(ctx.pinnedActions, ctx.path, ctx.onOpenWithPinned),
        { label: 'New folder', shortcut: SHORTCUT_KEYS.newFolder, onSelect: () => ctx.onNewFolder(ctx.path) },
        { label: 'New file', onSelect: () => ctx.onNewFile(ctx.path) },
        separator,
        { label: 'Paste', shortcut: SHORTCUT_KEYS.paste, onSelect: () => ctx.onPaste(ctx.path) },
        separator,
        { label: 'Open terminal here', onSelect: () => ctx.onOpenTerminal(ctx.path) },
        { label: 'Pin this folder', onSelect: () => ctx.onPin(target) },
        separator,
        { label: 'Properties', onSelect: () => ctx.onProperties(target) },
    ];
}
