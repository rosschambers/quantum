import { describe, it, expect } from 'vitest';
import type { FileEntry } from '@quantum/client';
import { PaneState } from './paneState.svelte';

/** Build a `FileEntry` with sensible defaults, overriding only what a test cares about. */
function entry(overrides: Partial<FileEntry> & { name: string }): FileEntry {
    return {
        name: overrides.name,
        path: overrides.path ?? `/${overrides.name}`,
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

/** Build a plain file `FileEntry` from an explicit path and name. */
function fileEntry(path: string, name: string): FileEntry {
    return entry({ name, path, kind: 'file' });
}

describe('PaneState construction', () => {
    it('seeds path and single-entry history', () => {
        const pane = new PaneState('/home/user');
        expect(pane.path).toBe('/home/user');
        expect(pane.history).toEqual(['/home/user']);
        expect(pane.historyIndex).toBe(0);
        expect(pane.canGoBack).toBe(false);
        expect(pane.canGoForward).toBe(false);
    });
});

describe('PaneState history', () => {
    it('navigate pushes onto history and moves the index', () => {
        const pane = new PaneState('/a');
        pane.navigate('/b');
        pane.navigate('/c');
        expect(pane.path).toBe('/c');
        expect(pane.history).toEqual(['/a', '/b', '/c']);
        expect(pane.historyIndex).toBe(2);
        expect(pane.canGoBack).toBe(true);
        expect(pane.canGoForward).toBe(false);
    });

    it('back moves within bounds without truncating', () => {
        const pane = new PaneState('/a');
        pane.navigate('/b');
        pane.navigate('/c');
        pane.back();
        expect(pane.path).toBe('/b');
        expect(pane.historyIndex).toBe(1);
        expect(pane.history).toEqual(['/a', '/b', '/c']);
        expect(pane.canGoBack).toBe(true);
        expect(pane.canGoForward).toBe(true);
    });

    it('forward moves within bounds', () => {
        const pane = new PaneState('/a');
        pane.navigate('/b');
        pane.back();
        pane.forward();
        expect(pane.path).toBe('/b');
        expect(pane.historyIndex).toBe(1);
        expect(pane.canGoForward).toBe(false);
    });

    it('navigate after back truncates forward entries', () => {
        const pane = new PaneState('/a');
        pane.navigate('/b');
        pane.navigate('/c');
        pane.back(); // back to /b
        pane.navigate('/d'); // truncates /c
        expect(pane.path).toBe('/d');
        expect(pane.history).toEqual(['/a', '/b', '/d']);
        expect(pane.historyIndex).toBe(2);
        expect(pane.canGoForward).toBe(false);
    });

    it('back is a no-op at the start of history', () => {
        const pane = new PaneState('/a');
        pane.back();
        expect(pane.path).toBe('/a');
        expect(pane.historyIndex).toBe(0);
    });

    it('forward is a no-op at the end of history', () => {
        const pane = new PaneState('/a');
        pane.navigate('/b');
        pane.forward();
        expect(pane.path).toBe('/b');
        expect(pane.historyIndex).toBe(1);
    });

    it('navigate clears selection and resets filter and deepSearch', () => {
        const pane = new PaneState('/a');
        pane.filter = 'query';
        pane.deepSearch = true;
        pane.selectOnly('/a/x');
        pane.navigate('/b');
        expect(pane.filter).toBe('');
        expect(pane.deepSearch).toBe(false);
        expect(pane.selection.size).toBe(0);
    });

    it('back clears selection', () => {
        const pane = new PaneState('/a');
        pane.navigate('/b');
        pane.selectOnly('/b/x');
        pane.back();
        expect(pane.selection.size).toBe(0);
    });
});

describe('PaneState up', () => {
    it('navigates to the parent directory', () => {
        const pane = new PaneState('/home/user');
        pane.up();
        expect(pane.path).toBe('/home');
    });

    it('stays at root when already at root', () => {
        const pane = new PaneState('/');
        pane.up();
        expect(pane.path).toBe('/');
    });

    it('does not push a duplicate history entry when already at root', () => {
        const pane = new PaneState('/');
        pane.up();
        expect(pane.path).toBe('/');
        expect(pane.history.length).toBe(1);
    });

    it('goes to root from a top-level directory', () => {
        const pane = new PaneState('/home');
        pane.up();
        expect(pane.path).toBe('/');
    });
});

describe('PaneState visibleEntries', () => {
    it('sorts folders before files, then by name ascending', () => {
        const pane = new PaneState('/a');
        pane.entries = [
            entry({ name: 'zeta.txt', kind: 'file' }),
            entry({ name: 'Alpha', kind: 'directory' }),
            entry({ name: 'beta.txt', kind: 'file' }),
            entry({ name: 'gamma', kind: 'directory' }),
        ];
        expect(pane.visibleEntries().map((item) => item.name)).toEqual([
            'Alpha',
            'gamma',
            'beta.txt',
            'zeta.txt',
        ]);
    });

    it('keeps folders first even when the sort direction is reversed', () => {
        const pane = new PaneState('/a');
        pane.entries = [
            entry({ name: 'Alpha', kind: 'directory' }),
            entry({ name: 'gamma', kind: 'directory' }),
            entry({ name: 'beta.txt', kind: 'file' }),
            entry({ name: 'zeta.txt', kind: 'file' }),
        ];
        pane.toggleSort('name'); // same column, flips to descending
        expect(pane.sortDirection).toBe(-1);
        expect(pane.visibleEntries().map((item) => item.name)).toEqual([
            'gamma',
            'Alpha',
            'zeta.txt',
            'beta.txt',
        ]);
    });

    it('filters by case-insensitive substring on name', () => {
        const pane = new PaneState('/a');
        pane.entries = [
            entry({ name: 'Report.pdf', kind: 'file' }),
            entry({ name: 'notes.txt', kind: 'file' }),
            entry({ name: 'reports', kind: 'directory' }),
        ];
        pane.filter = 'REPORT';
        expect(pane.visibleEntries().map((item) => item.name)).toEqual([
            'reports',
            'Report.pdf',
        ]);
    });

    it('does not filter locally when deepSearch is active', () => {
        const pane = new PaneState('/a');
        pane.entries = [
            entry({ name: 'one.txt', kind: 'file' }),
            entry({ name: 'two.txt', kind: 'file' }),
        ];
        pane.deepSearch = true;
        pane.filter = 'nomatch';
        expect(pane.visibleEntries().map((item) => item.name)).toEqual([
            'one.txt',
            'two.txt',
        ]);
    });

    it('sorts by size using recursive_size when present', () => {
        const pane = new PaneState('/a');
        pane.entries = [
            entry({ name: 'big.txt', kind: 'file', size: 300 }),
            entry({ name: 'small.txt', kind: 'file', size: 100 }),
            entry({ name: 'folder', kind: 'directory', size: 0, recursive_size: 999 }),
        ];
        pane.toggleSort('size'); // ascending on size, folders still first
        expect(pane.visibleEntries().map((item) => item.name)).toEqual([
            'folder',
            'small.txt',
            'big.txt',
        ]);
    });

    it('sorts by mtime', () => {
        const pane = new PaneState('/a');
        pane.entries = [
            entry({ name: 'new.txt', kind: 'file', modified_epoch_seconds: 300 }),
            entry({ name: 'old.txt', kind: 'file', modified_epoch_seconds: 100 }),
        ];
        pane.toggleSort('mtime');
        expect(pane.visibleEntries().map((item) => item.name)).toEqual([
            'old.txt',
            'new.txt',
        ]);
    });
});

describe('PaneState toggleSort', () => {
    it('flips direction on the same column', () => {
        const pane = new PaneState('/a');
        expect(pane.sortBy).toBe('name');
        expect(pane.sortDirection).toBe(1);
        pane.toggleSort('name');
        expect(pane.sortDirection).toBe(-1);
        pane.toggleSort('name');
        expect(pane.sortDirection).toBe(1);
    });

    it('resets to ascending on a new column', () => {
        const pane = new PaneState('/a');
        pane.toggleSort('name'); // now descending
        pane.toggleSort('size'); // new column resets to ascending
        expect(pane.sortBy).toBe('size');
        expect(pane.sortDirection).toBe(1);
    });
});

describe('PaneState selection', () => {
    it('selectOnly replaces the selection with a single path', () => {
        const pane = new PaneState('/a');
        pane.selectOnly('/a/one');
        pane.selectOnly('/a/two');
        expect([...pane.selection]).toEqual(['/a/two']);
    });

    it('toggleSelect adds and removes a path', () => {
        const pane = new PaneState('/a');
        pane.toggleSelect('/a/one');
        expect(pane.selection.has('/a/one')).toBe(true);
        pane.toggleSelect('/a/one');
        expect(pane.selection.has('/a/one')).toBe(false);
    });

    it('clearSelection empties the selection', () => {
        const pane = new PaneState('/a');
        pane.toggleSelect('/a/one');
        pane.toggleSelect('/a/two');
        pane.clearSelection();
        expect(pane.selection.size).toBe(0);
    });

    it('selectRange selects over visible order', () => {
        const pane = new PaneState('/a');
        pane.entries = [
            entry({ name: 'dir', kind: 'directory', path: '/a/dir' }),
            entry({ name: 'apple.txt', kind: 'file', path: '/a/apple.txt' }),
            entry({ name: 'banana.txt', kind: 'file', path: '/a/banana.txt' }),
            entry({ name: 'cherry.txt', kind: 'file', path: '/a/cherry.txt' }),
        ];
        // Visible order is folders-first then name ascending:
        // dir, apple.txt, banana.txt, cherry.txt
        pane.selectRange(1, 2);
        expect([...pane.selection].sort()).toEqual(['/a/apple.txt', '/a/banana.txt']);
    });

    it('selectRange works regardless of index order', () => {
        const pane = new PaneState('/a');
        pane.entries = [
            entry({ name: 'apple.txt', kind: 'file', path: '/a/apple.txt' }),
            entry({ name: 'banana.txt', kind: 'file', path: '/a/banana.txt' }),
            entry({ name: 'cherry.txt', kind: 'file', path: '/a/cherry.txt' }),
        ];
        pane.selectRange(2, 0);
        expect([...pane.selection].sort()).toEqual([
            '/a/apple.txt',
            '/a/banana.txt',
            '/a/cherry.txt',
        ]);
    });

    it('selectAll selects exactly the visible entries', () => {
        const pane = new PaneState('/');
        pane.entries = [fileEntry('/a', 'a'), fileEntry('/b', 'b'), fileEntry('/c', 'c')];
        pane.selectAll();
        expect([...pane.selection].sort()).toEqual(['/a', '/b', '/c']);
    });
});
