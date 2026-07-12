// Per-pane reactive state for the file-explorer view, written as a Svelte 5
// runes class in a `.svelte.ts` module. This is PURE state: it holds the
// current directory, navigation history, listed entries, selection, and the
// filter/sort configuration, and derives the visible (filtered and sorted)
// entries. It never performs IPC — loading entries is the App's job — which
// keeps it unit-testable without mocking the client.

import type { FileEntry } from '@quantum/client';
import { parentOf } from './path';

/** The column a pane sorts its entries by. */
export type SortColumn = 'name' | 'size' | 'mtime';

/** Sort direction: 1 for ascending, -1 for descending. */
export type SortDirection = 1 | -1;

export class PaneState {
    /** The directory currently shown in the pane. */
    path = $state('');
    /** The full navigation history stack. */
    history = $state<string[]>([]);
    /** The index of the current path within `history`. */
    historyIndex = $state(0);
    /** The raw entries for the current directory (or search results). */
    entries = $state<FileEntry[]>([]);
    /** The set of selected entry paths. Reassigned on change to stay reactive. */
    selection = $state<Set<string>>(new Set());
    /** The local filter substring applied to entry names. */
    filter = $state('');
    /** Whether the pane is showing recursive search results rather than a listing. */
    deepSearch = $state(false);
    /** The active sort column. */
    sortBy = $state<SortColumn>('name');
    /** The active sort direction. */
    sortDirection = $state<SortDirection>(1);
    /** Whether a listing or search is in flight. */
    loading = $state(false);

    /** True when there is an earlier entry to navigate back to. */
    canGoBack = $derived(this.historyIndex > 0);
    /** True when there is a later entry to navigate forward to. */
    canGoForward = $derived(this.historyIndex < this.history.length - 1);

    constructor(initialPath: string) {
        this.path = initialPath;
        this.history = [initialPath];
        this.historyIndex = 0;
    }

    /**
     * Navigate to a new path. Any forward history is truncated, the new path is
     * pushed and becomes current, selection is cleared, and the filter and
     * deepSearch state reset.
     */
    navigate(path: string): void {
        this.history = this.history.slice(0, this.historyIndex + 1);
        this.history.push(path);
        this.historyIndex = this.history.length - 1;
        this.path = path;
        this.clearSelection();
        this.filter = '';
        this.deepSearch = false;
    }

    /** Move one step back in history without truncating; clears selection. */
    back(): void {
        if (!this.canGoBack) {
            return;
        }
        this.historyIndex -= 1;
        this.path = this.history[this.historyIndex];
        this.clearSelection();
    }

    /** Move one step forward in history without truncating; clears selection. */
    forward(): void {
        if (!this.canGoForward) {
            return;
        }
        this.historyIndex += 1;
        this.path = this.history[this.historyIndex];
        this.clearSelection();
    }

    /**
     * Navigate to the parent directory of the current path. A no-op at the
     * filesystem root, where the parent equals the current path, so `up()` does
     * not push a duplicate history entry.
     */
    up(): void {
        const parent = parentOf(this.path);
        if (parent === this.path) {
            return;
        }
        this.navigate(parent);
    }

    /**
     * The entries to render: filtered by the case-insensitive name substring
     * (unless deepSearch is active, in which case entries are already search
     * results and are not re-filtered), then sorted folders-first and by the
     * active column and direction.
     */
    visibleEntries(): FileEntry[] {
        const filterText = this.filter.toLowerCase();
        const filtered =
            this.deepSearch || filterText === ''
                ? this.entries.slice()
                : this.entries.filter((item) => item.name.toLowerCase().includes(filterText));
        filtered.sort((a, b) => this.compareEntries(a, b));
        return filtered;
    }

    /** Comparator: directories always precede files, then by the active column. */
    private compareEntries(a: FileEntry, b: FileEntry): number {
        const aIsDirectory = a.kind === 'directory';
        const bIsDirectory = b.kind === 'directory';
        if (aIsDirectory !== bIsDirectory) {
            return aIsDirectory ? -1 : 1;
        }
        let comparison: number;
        if (this.sortBy === 'size') {
            comparison = (a.recursive_size ?? a.size) - (b.recursive_size ?? b.size);
        } else if (this.sortBy === 'mtime') {
            comparison = a.modified_epoch_seconds - b.modified_epoch_seconds;
        } else {
            comparison = a.name.localeCompare(b.name);
        }
        return comparison * this.sortDirection;
    }

    /**
     * Toggle sorting. Sorting by the current column flips direction; a new
     * column resets to ascending.
     */
    toggleSort(column: SortColumn): void {
        if (this.sortBy === column) {
            this.sortDirection = this.sortDirection === 1 ? -1 : 1;
        } else {
            this.sortBy = column;
            this.sortDirection = 1;
        }
    }

    /** Replace the selection with a single path. */
    selectOnly(path: string): void {
        this.selection = new Set([path]);
    }

    /** Add a path to the selection if absent, otherwise remove it. */
    toggleSelect(path: string): void {
        const next = new Set(this.selection);
        if (next.has(path)) {
            next.delete(path);
        } else {
            next.add(path);
        }
        this.selection = next;
    }

    /**
     * Add every entry between two indices (inclusive) in visible order to the
     * selection. The indices may be given in either order.
     */
    selectRange(fromVisibleIndex: number, toVisibleIndex: number): void {
        const visible = this.visibleEntries();
        const start = Math.min(fromVisibleIndex, toVisibleIndex);
        const end = Math.max(fromVisibleIndex, toVisibleIndex);
        const next = new Set(this.selection);
        for (let index = start; index <= end; index += 1) {
            const item = visible[index];
            if (item) {
                next.add(item.path);
            }
        }
        this.selection = next;
    }

    /** Empty the selection. */
    clearSelection(): void {
        this.selection = new Set();
    }

    /** Select every currently visible entry. */
    selectAll(): void {
        this.selection = new Set(this.visibleEntries().map((entry) => entry.path));
    }
}
