import { describe, it, expect, vi } from 'vitest';
import type { FileEntry, MenuItem } from '@quantum/client';
import {
    buildEntryMenu,
    buildBackgroundMenu,
    type EntryMenuContext,
    type BackgroundMenuContext,
} from './menus';
import { SHORTCUT_KEYS } from './shortcuts';

/** Build a `FileEntry` with sensible defaults, overriding only what a test cares about. */
function entry(overrides: Partial<FileEntry> & { name: string }): FileEntry {
    return {
        name: overrides.name,
        path: overrides.path ?? `/home/user/${overrides.name}`,
        kind: overrides.kind ?? 'file',
        size: overrides.size ?? 0,
        recursive_size: overrides.recursive_size ?? null,
        modified_epoch_seconds: overrides.modified_epoch_seconds ?? 0,
        owner: overrides.owner ?? 'user',
        permissions: overrides.permissions ?? 'rw-r--r--',
        permission_class: overrides.permission_class ?? 'normal',
        symlink_target: overrides.symlink_target ?? null,
        content_kind: overrides.content_kind ?? 'other',
    };
}

/** The visible (non-separator) labels of a menu, in order. */
function labels(items: MenuItem[]): string[] {
    return items.filter((item) => !item.separator).map((item) => item.label);
}

/** Find a menu item by its exact label. */
function item(items: MenuItem[], label: string): MenuItem | undefined {
    return items.find((menuItem) => menuItem.label === label);
}

/** Build an entry-menu context whose callbacks are all spies, overridable per test. */
function entryContext(overrides: Partial<EntryMenuContext> = {}): EntryMenuContext {
    const target = overrides.entry ?? entry({ name: 'report.txt' });
    return {
        entry: target,
        selectionPaths: overrides.selectionPaths ?? [target.path],
        path: overrides.path ?? '/home/user',
        isPinned: overrides.isPinned ?? (() => false),
        applications: overrides.applications ?? [],
        onOpen: overrides.onOpen ?? vi.fn(),
        onOpenWithPicker: overrides.onOpenWithPicker ?? vi.fn(),
        onOpenTerminal: overrides.onOpenTerminal ?? vi.fn(),
        onClipboard: overrides.onClipboard ?? vi.fn(),
        onPaste: overrides.onPaste ?? vi.fn(),
        onOperation: overrides.onOperation ?? vi.fn(),
        onRename: overrides.onRename ?? vi.fn(),
        onPin: overrides.onPin ?? vi.fn(),
        onUnpin: overrides.onUnpin ?? vi.fn(),
        onCopyPath: overrides.onCopyPath ?? vi.fn(),
        onProperties: overrides.onProperties ?? vi.fn(),
        pinnedActions: overrides.pinnedActions ?? [],
        onOpenWithPinned: overrides.onOpenWithPinned ?? vi.fn(),
    };
}

describe('buildEntryMenu order and membership', () => {
    it('lists a plain file entry in the specified order without Pin', () => {
        const items = buildEntryMenu(entryContext({ entry: entry({ name: 'report.txt' }) }));
        expect(labels(items)).toEqual([
            'Open',
            'Preview',
            'Open with...',
            'Open terminal here',
            'Cut',
            'Copy',
            'Paste',
            'Duplicate',
            'Rename',
            'Compress',
            'Copy path',
            'Move to trash',
            'Delete permanently',
            'Properties',
        ]);
    });

    it('adds Pin to sidebar for an unpinned directory', () => {
        const directory = entry({ name: 'photos', kind: 'directory' });
        const items = buildEntryMenu(entryContext({ entry: directory, isPinned: () => false }));
        expect(labels(items)).toContain('Pin to sidebar');
        expect(labels(items)).not.toContain('Unpin from sidebar');
    });

    it('adds Unpin from sidebar for a pinned directory', () => {
        const directory = entry({ name: 'photos', kind: 'directory' });
        const items = buildEntryMenu(entryContext({ entry: directory, isPinned: () => true }));
        expect(labels(items)).toContain('Unpin from sidebar');
        expect(labels(items)).not.toContain('Pin to sidebar');
    });

    it('offers Extract instead of Compress for an archive', () => {
        const archive = entry({ name: 'bundle.tar.zst', content_kind: 'archive' });
        const items = buildEntryMenu(entryContext({ entry: archive }));
        expect(labels(items)).toContain('Extract');
        expect(labels(items)).not.toContain('Compress');
    });
});

describe('buildEntryMenu actions', () => {
    it('Open invokes onOpen with the entry', () => {
        const target = entry({ name: 'report.txt' });
        const onOpen = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, onOpen }));
        item(items, 'Open')?.onSelect?.();
        expect(onOpen).toHaveBeenCalledWith(target);
    });

    it('Open with... invokes onOpenWithPicker with the entry', () => {
        const target = entry({ name: 'report.txt' });
        const onOpenWithPicker = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, onOpenWithPicker }));
        item(items, 'Open with...')?.onSelect?.();
        expect(onOpenWithPicker).toHaveBeenCalledWith(target);
    });

    it('Open terminal here uses the entry path for a directory', () => {
        const directory = entry({ name: 'photos', kind: 'directory', path: '/home/user/photos' });
        const onOpenTerminal = vi.fn();
        const items = buildEntryMenu(
            entryContext({ entry: directory, path: '/home/user', onOpenTerminal }),
        );
        item(items, 'Open terminal here')?.onSelect?.();
        expect(onOpenTerminal).toHaveBeenCalledWith('/home/user/photos');
    });

    it('Open terminal here uses the current directory for a file', () => {
        const file = entry({ name: 'report.txt', path: '/home/user/report.txt' });
        const onOpenTerminal = vi.fn();
        const items = buildEntryMenu(
            entryContext({ entry: file, path: '/home/user', onOpenTerminal }),
        );
        item(items, 'Open terminal here')?.onSelect?.();
        expect(onOpenTerminal).toHaveBeenCalledWith('/home/user');
    });

    it('Cut and Copy pass the resolved paths to onClipboard', () => {
        const target = entry({ name: 'report.txt' });
        const onClipboard = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, onClipboard }));
        item(items, 'Cut')?.onSelect?.();
        item(items, 'Copy')?.onSelect?.();
        expect(onClipboard).toHaveBeenNthCalledWith(1, 'cut', [target.path]);
        expect(onClipboard).toHaveBeenNthCalledWith(2, 'copy', [target.path]);
    });

    it('Paste invokes onPaste with the current directory', () => {
        const onPaste = vi.fn();
        const items = buildEntryMenu(entryContext({ path: '/home/user', onPaste }));
        item(items, 'Paste')?.onSelect?.();
        expect(onPaste).toHaveBeenCalledWith('/home/user');
    });

    it('Duplicate dispatches a duplicate operation for the entry path', () => {
        const target = entry({ name: 'report.txt', path: '/home/user/report.txt' });
        const onOperation = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, onOperation }));
        item(items, 'Duplicate')?.onSelect?.();
        expect(onOperation).toHaveBeenCalledWith({ kind: 'duplicate', path: '/home/user/report.txt' });
    });

    it('Rename invokes onRename with the entry', () => {
        const target = entry({ name: 'report.txt' });
        const onRename = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, onRename }));
        item(items, 'Rename')?.onSelect?.();
        expect(onRename).toHaveBeenCalledWith(target);
    });

    it('Compress builds a tar.zst destination in the current directory', () => {
        const target = entry({ name: 'report', path: '/home/user/report' });
        const onOperation = vi.fn();
        const items = buildEntryMenu(
            entryContext({ entry: target, path: '/home/user', onOperation }),
        );
        item(items, 'Compress')?.onSelect?.();
        expect(onOperation).toHaveBeenCalledWith({
            kind: 'compress',
            paths: ['/home/user/report'],
            destination: '/home/user/report.tar.zst',
        });
    });

    it('Compress builds a root-level destination without a double slash', () => {
        const target = entry({ name: 'report', path: '/report' });
        const onOperation = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, path: '/', onOperation }));
        item(items, 'Compress')?.onSelect?.();
        expect(onOperation).toHaveBeenCalledWith({
            kind: 'compress',
            paths: ['/report'],
            destination: '/report.tar.zst',
        });
    });

    it('Extract dispatches an extract operation for the archive path', () => {
        const archive = entry({ name: 'bundle.tar.zst', content_kind: 'archive', path: '/home/user/bundle.tar.zst' });
        const onOperation = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: archive, onOperation }));
        item(items, 'Extract')?.onSelect?.();
        expect(onOperation).toHaveBeenCalledWith({ kind: 'extract', path: '/home/user/bundle.tar.zst' });
    });

    it('Pin to sidebar invokes onPin with the directory entry', () => {
        const directory = entry({ name: 'photos', kind: 'directory' });
        const onPin = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: directory, isPinned: () => false, onPin }));
        item(items, 'Pin to sidebar')?.onSelect?.();
        expect(onPin).toHaveBeenCalledWith(directory);
    });

    it('Unpin from sidebar invokes onUnpin with the directory path', () => {
        const directory = entry({ name: 'photos', kind: 'directory', path: '/home/user/photos' });
        const onUnpin = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: directory, isPinned: () => true, onUnpin }));
        item(items, 'Unpin from sidebar')?.onSelect?.();
        expect(onUnpin).toHaveBeenCalledWith('/home/user/photos');
    });

    it('Copy path invokes onCopyPath with the entry path', () => {
        const target = entry({ name: 'report.txt', path: '/home/user/report.txt' });
        const onCopyPath = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, onCopyPath }));
        item(items, 'Copy path')?.onSelect?.();
        expect(onCopyPath).toHaveBeenCalledWith('/home/user/report.txt');
    });

    it('Move to trash and Delete permanently are danger items', () => {
        const items = buildEntryMenu(entryContext());
        expect(item(items, 'Move to trash')?.danger).toBe(true);
        expect(item(items, 'Delete permanently')?.danger).toBe(true);
    });

    it('Properties invokes onProperties with the entry', () => {
        const target = entry({ name: 'report.txt' });
        const onProperties = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, onProperties }));
        item(items, 'Properties')?.onSelect?.();
        expect(onProperties).toHaveBeenCalledWith(target);
    });
});

describe('buildEntryMenu shortcut accelerators', () => {
    it('entry menu items carry keyboard shortcuts', () => {
        const items = buildEntryMenu(entryContext());
        expect(item(items, 'Cut')?.shortcut).toBe(SHORTCUT_KEYS.cut);
        expect(item(items, 'Copy')?.shortcut).toBe(SHORTCUT_KEYS.copy);
        expect(item(items, 'Paste')?.shortcut).toBe(SHORTCUT_KEYS.paste);
        expect(item(items, 'Duplicate')?.shortcut).toBe(SHORTCUT_KEYS.duplicate);
        expect(item(items, 'Rename')?.shortcut).toBe(SHORTCUT_KEYS.rename);
        expect(item(items, 'Move to trash')?.shortcut).toBe(SHORTCUT_KEYS.trash);
        expect(item(items, 'Delete permanently')?.shortcut).toBe(SHORTCUT_KEYS.deletePermanent);
    });

    it('entry menu items without a keybind carry no shortcut', () => {
        const items = buildEntryMenu(entryContext());
        expect(item(items, 'Open')?.shortcut).toBeUndefined();
        expect(item(items, 'Open with...')?.shortcut).toBeUndefined();
        expect(item(items, 'Open terminal here')?.shortcut).toBeUndefined();
        expect(item(items, 'Copy path')?.shortcut).toBeUndefined();
        expect(item(items, 'Properties')?.shortcut).toBeUndefined();
    });
});

describe('buildBackgroundMenu shortcut accelerators', () => {
    it('New folder and Paste carry their shortcuts', () => {
        const items = buildBackgroundMenu(backgroundContext());
        expect(item(items, 'New folder')?.shortcut).toBe(SHORTCUT_KEYS.newFolder);
        expect(item(items, 'Paste')?.shortcut).toBe(SHORTCUT_KEYS.paste);
    });

    it('background items without a keybind carry no shortcut', () => {
        const items = buildBackgroundMenu(backgroundContext());
        expect(item(items, 'New file')?.shortcut).toBeUndefined();
        expect(item(items, 'Open terminal here')?.shortcut).toBeUndefined();
        expect(item(items, 'Pin this folder')?.shortcut).toBeUndefined();
        expect(item(items, 'Properties')?.shortcut).toBeUndefined();
    });
});

describe('buildEntryMenu multi-select path resolution', () => {
    it('acts on every selected path when the target is within a multi-selection', () => {
        const target = entry({ name: 'a.txt', path: '/home/user/a.txt' });
        const selectionPaths = ['/home/user/a.txt', '/home/user/b.txt'];
        const onOperation = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, selectionPaths, onOperation }));
        item(items, 'Move to trash')?.onSelect?.();
        expect(onOperation).toHaveBeenCalledWith({ kind: 'trash', paths: selectionPaths });
    });

    it('acts on only the target when it is not part of the selection', () => {
        const target = entry({ name: 'a.txt', path: '/home/user/a.txt' });
        const selectionPaths = ['/home/user/b.txt', '/home/user/c.txt'];
        const onOperation = vi.fn();
        const items = buildEntryMenu(entryContext({ entry: target, selectionPaths, onOperation }));
        item(items, 'Delete permanently')?.onSelect?.();
        expect(onOperation).toHaveBeenCalledWith({ kind: 'delete', paths: ['/home/user/a.txt'] });
    });

    it('acts on only the target when the selection is a single item', () => {
        const target = entry({ name: 'a.txt', path: '/home/user/a.txt' });
        const onClipboard = vi.fn();
        const items = buildEntryMenu(
            entryContext({ entry: target, selectionPaths: ['/home/user/a.txt'], onClipboard }),
        );
        item(items, 'Copy')?.onSelect?.();
        expect(onClipboard).toHaveBeenCalledWith('copy', ['/home/user/a.txt']);
    });
});

/** Build a background-menu context whose callbacks are all spies. */
function backgroundContext(overrides: Partial<BackgroundMenuContext> = {}): BackgroundMenuContext {
    return {
        path: overrides.path ?? '/home/user',
        onNewFolder: overrides.onNewFolder ?? vi.fn(),
        onNewFile: overrides.onNewFile ?? vi.fn(),
        onPaste: overrides.onPaste ?? vi.fn(),
        onOpenTerminal: overrides.onOpenTerminal ?? vi.fn(),
        onPin: overrides.onPin ?? vi.fn(),
        onProperties: overrides.onProperties ?? vi.fn(),
        pinnedActions: overrides.pinnedActions ?? [],
        onOpenWithPinned: overrides.onOpenWithPinned ?? vi.fn(),
    };
}

describe('buildBackgroundMenu', () => {
    it('lists the background actions in the specified order', () => {
        const items = buildBackgroundMenu(backgroundContext());
        expect(labels(items)).toEqual([
            'New folder',
            'New file',
            'Paste',
            'Open terminal here',
            'Pin this folder',
            'Properties',
        ]);
    });

    it('New folder and New file target the current directory', () => {
        const onNewFolder = vi.fn();
        const onNewFile = vi.fn();
        const items = buildBackgroundMenu(
            backgroundContext({ path: '/home/user', onNewFolder, onNewFile }),
        );
        item(items, 'New folder')?.onSelect?.();
        item(items, 'New file')?.onSelect?.();
        expect(onNewFolder).toHaveBeenCalledWith('/home/user');
        expect(onNewFile).toHaveBeenCalledWith('/home/user');
    });

    it('Paste and Open terminal here target the current directory', () => {
        const onPaste = vi.fn();
        const onOpenTerminal = vi.fn();
        const items = buildBackgroundMenu(
            backgroundContext({ path: '/home/user', onPaste, onOpenTerminal }),
        );
        item(items, 'Paste')?.onSelect?.();
        item(items, 'Open terminal here')?.onSelect?.();
        expect(onPaste).toHaveBeenCalledWith('/home/user');
        expect(onOpenTerminal).toHaveBeenCalledWith('/home/user');
    });

    it('Pin this folder pins the current directory with its base name', () => {
        const onPin = vi.fn();
        const items = buildBackgroundMenu(backgroundContext({ path: '/home/user/photos', onPin }));
        item(items, 'Pin this folder')?.onSelect?.();
        expect(onPin).toHaveBeenCalledWith({ path: '/home/user/photos', name: 'photos' });
    });

    it('Properties targets the current directory', () => {
        const onProperties = vi.fn();
        const items = buildBackgroundMenu(backgroundContext({ path: '/home/user/photos', onProperties }));
        item(items, 'Properties')?.onSelect?.();
        expect(onProperties).toHaveBeenCalledWith({ path: '/home/user/photos', name: 'photos' });
    });
});

describe('pinned actions', () => {
    const pins = [
        { desktop_id: 'firefox.desktop', label: 'Open with Firefox' },
        { desktop_id: 'code.desktop', label: 'Open with Visual Studio Code' },
    ];

    it('prepends pinned items above Open on the entry menu', () => {
        const target = entry({ name: 'notes.txt', path: '/home/user/notes.txt' });
        const items = buildEntryMenu(entryContext({ entry: target, pinnedActions: pins }));
        expect(labels(items).slice(0, 3)).toEqual([
            'Open with Firefox',
            'Open with Visual Studio Code',
            'Open',
        ]);
    });

    it('entry pinned item opens the entry path with the desktop id', () => {
        const onOpenWithPinned = vi.fn();
        const target = entry({ name: 'notes.txt', path: '/home/user/notes.txt' });
        const items = buildEntryMenu(
            entryContext({ entry: target, pinnedActions: pins, onOpenWithPinned }),
        );
        item(items, 'Open with Firefox')?.onSelect?.();
        expect(onOpenWithPinned).toHaveBeenCalledWith('firefox.desktop', '/home/user/notes.txt');
    });

    it('leaves the entry menu unchanged when no pins', () => {
        const items = buildEntryMenu(entryContext({ pinnedActions: [] }));
        expect(labels(items)[0]).toBe('Open');
    });

    it('prepends pinned items above New folder on the background menu', () => {
        const items = buildBackgroundMenu(
            backgroundContext({ path: '/home/user/project', pinnedActions: pins }),
        );
        expect(labels(items).slice(0, 3)).toEqual([
            'Open with Firefox',
            'Open with Visual Studio Code',
            'New folder',
        ]);
    });

    it('background pinned item opens the current directory', () => {
        const onOpenWithPinned = vi.fn();
        const items = buildBackgroundMenu(
            backgroundContext({ path: '/home/user/project', pinnedActions: pins, onOpenWithPinned }),
        );
        item(items, 'Open with Visual Studio Code')?.onSelect?.();
        expect(onOpenWithPinned).toHaveBeenCalledWith('code.desktop', '/home/user/project');
    });
});
