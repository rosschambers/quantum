import { describe, it, expect } from 'vitest';
import type { FileEntry } from '@quantum/client';
import { PaneState } from './paneState.svelte';
import type { ListHeader, ListItem } from './paneState.svelte';

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

    it('hides dotfiles when showHidden is false', () => {
        const pane = new PaneState('/');
        pane.entries = [fileEntry('/.git', '.git'), fileEntry('/a', 'a')];
        pane.showHidden = false;
        expect(pane.visibleEntries().map((e) => e.name)).toEqual(['a']);
    });

    it('shows dotfiles when showHidden is true', () => {
        const pane = new PaneState('/');
        pane.entries = [fileEntry('/.git', '.git'), fileEntry('/a', 'a')];
        pane.showHidden = true;
        expect(pane.visibleEntries().map((e) => e.name).sort()).toEqual(['.git', 'a']);
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

describe('PaneState sizing', () => {
    it('starts with an empty sizing set', () => {
        const pane = new PaneState('/a');
        expect(pane.sizing.size).toBe(0);
    });

    it('markSizing replaces the sizing set with the given paths', () => {
        const pane = new PaneState('/a');
        pane.markSizing(['/a', '/b']);
        expect([...pane.sizing].sort()).toEqual(['/a', '/b']);
    });

    it('markSizing replaces rather than merges an existing set', () => {
        const pane = new PaneState('/a');
        pane.markSizing(['/a', '/b']);
        pane.markSizing(['/c']);
        expect([...pane.sizing]).toEqual(['/c']);
    });

    it('setSizeComplete removes a single path', () => {
        const pane = new PaneState('/a');
        pane.markSizing(['/a', '/b']);
        pane.setSizeComplete('/a');
        expect([...pane.sizing]).toEqual(['/b']);
    });

    it('addSizing adds a path not yet tracked', () => {
        const pane = new PaneState('/a');
        pane.markSizing(['/a']);
        pane.addSizing('/c');
        expect([...pane.sizing].sort()).toEqual(['/a', '/c']);
    });

    it('clearSizing empties the sizing set', () => {
        const pane = new PaneState('/a');
        pane.markSizing(['/a', '/b']);
        pane.clearSizing();
        expect(pane.sizing.size).toBe(0);
    });

    it('mutations reassign a new Set reference for reactivity', () => {
        const pane = new PaneState('/a');
        pane.markSizing(['/a', '/b']);
        const before = pane.sizing;
        pane.setSizeComplete('/a');
        expect(pane.sizing).not.toBe(before);
    });
});

describe('PaneState grouped deep-search ordering', () => {
    it('leaves visibleEntries in natural order when not deep-searching', () => {
        const pane = new PaneState('/search');
        pane.entries = [
            entry({ name: 'zeta.txt', path: '/search/sub/zeta.txt' }),
            entry({ name: 'alpha.txt', path: '/search/alpha.txt' }),
        ];
        pane.filter = 'txt';
        expect(pane.deepSearch).toBe(false);
        expect(pane.visibleEntries().map((item) => item.path)).toEqual([
            '/search/alpha.txt',
            '/search/sub/zeta.txt',
        ]);
    });

    it('leaves visibleEntries in natural order when deep-searching with an empty filter', () => {
        const pane = new PaneState('/search');
        pane.entries = [
            entry({ name: 'zeta.txt', path: '/search/sub/zeta.txt' }),
            entry({ name: 'alpha.txt', path: '/search/alpha.txt' }),
        ];
        pane.deepSearch = true;
        pane.filter = '   ';
        expect(pane.visibleEntries().map((item) => item.path)).toEqual([
            '/search/alpha.txt',
            '/search/sub/zeta.txt',
        ]);
    });

    it('groups by containing folder, ordered alphabetically, root group first', () => {
        const pane = new PaneState('/search');
        pane.deepSearch = true;
        pane.filter = 'txt';
        pane.entries = [
            entry({ name: 'nested.txt', path: '/search/sub/nested.txt' }),
            entry({ name: 'root.txt', path: '/search/root.txt' }),
            entry({ name: 'file.txt', path: '/search/beta/file.txt' }),
        ];
        expect(pane.visibleEntries().map((item) => item.path)).toEqual([
            '/search/root.txt',
            '/search/beta/file.txt',
            '/search/sub/nested.txt',
        ]);
    });

    it('preserves the current name sort order within each group', () => {
        const pane = new PaneState('/search');
        pane.deepSearch = true;
        pane.filter = 'txt';
        pane.entries = [
            entry({ name: 'zeta.txt', path: '/search/sub/zeta.txt' }),
            entry({ name: 'alpha.txt', path: '/search/sub/alpha.txt' }),
        ];
        expect(pane.visibleEntries().map((item) => item.name)).toEqual([
            'alpha.txt',
            'zeta.txt',
        ]);
    });

    it('preserves the current size sort order within each group', () => {
        const pane = new PaneState('/search');
        pane.deepSearch = true;
        pane.filter = 'txt';
        pane.entries = [
            entry({ name: 'big.txt', path: '/search/sub/big.txt', size: 500 }),
            entry({ name: 'small.txt', path: '/search/sub/small.txt', size: 10 }),
        ];
        pane.toggleSort('size');
        expect(pane.visibleEntries().map((item) => item.name)).toEqual([
            'small.txt',
            'big.txt',
        ]);
    });
});

describe('PaneState listItems', () => {
    function entriesOf(items: ListItem[]): string[] {
        return items
            .filter((item): item is { kind: 'entry'; entry: FileEntry } => item.kind === 'entry')
            .map((item) => item.entry.path);
    }

    function headersOf(items: ListItem[]): ListHeader[] {
        return items.filter((item): item is ListHeader => item.kind === 'header');
    }

    it('returns entries only, with no headers, when not grouped', () => {
        const pane = new PaneState('/a');
        pane.entries = [
            entry({ name: 'alpha.txt', path: '/a/alpha.txt' }),
            entry({ name: 'beta.txt', path: '/a/beta.txt' }),
        ];
        const items = pane.listItems();
        expect(headersOf(items)).toEqual([]);
        expect(entriesOf(items)).toEqual(['/a/alpha.txt', '/a/beta.txt']);
    });

    it('inserts one header per group, holding the absolute group path', () => {
        const pane = new PaneState('/search');
        pane.deepSearch = true;
        pane.filter = 'txt';
        pane.entries = [
            entry({ name: 'nested.txt', path: '/search/sub/nested.txt' }),
            entry({ name: 'root.txt', path: '/search/root.txt' }),
            entry({ name: 'file.txt', path: '/search/beta/file.txt' }),
        ];
        const items = pane.listItems();
        expect(headersOf(items).map((header) => header.path)).toEqual([
            '/search',
            '/search/beta',
            '/search/sub',
        ]);
        expect(items.map((item) => (item.kind === 'header' ? 'H' : item.entry.name))).toEqual([
            'H',
            'root.txt',
            'H',
            'file.txt',
            'H',
            'nested.txt',
        ]);
    });

    it('still emits exactly one header when every result shares a single group', () => {
        const pane = new PaneState('/search');
        pane.deepSearch = true;
        pane.filter = 'txt';
        pane.entries = [
            entry({ name: 'alpha.txt', path: '/search/sub/alpha.txt' }),
            entry({ name: 'beta.txt', path: '/search/sub/beta.txt' }),
        ];
        const items = pane.listItems();
        expect(headersOf(items)).toEqual([{ kind: 'header', path: '/search/sub' }]);
        expect(entriesOf(items)).toEqual(['/search/sub/alpha.txt', '/search/sub/beta.txt']);
    });

    it('gives the root-level group a header at the search root path', () => {
        const pane = new PaneState('/search');
        pane.deepSearch = true;
        pane.filter = 'txt';
        pane.entries = [entry({ name: 'root.txt', path: '/search/root.txt' })];
        const items = pane.listItems();
        expect(headersOf(items)).toEqual([{ kind: 'header', path: '/search' }]);
    });
});
