import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import type { FileEntry, FilesEvent, MenuItem } from '@quantum/client';
import App from './App.svelte';
import type { FilesIpc } from './lib/ipc';

// The App calls `openContextMenu` from `@quantum/client` on right-click. Mock it
// so we can assert the call and reach into the built menu items without laying
// out a real DOM popover. Keep every other export (createClient, types) real via
// importOriginal so `ipc.ts` still resolves its `createClient` import.
const { openContextMenuMock } = vi.hoisted(() => ({ openContextMenuMock: vi.fn() }));
vi.mock('@quantum/client', async (importOriginal) => {
    const actual = await importOriginal<Record<string, unknown>>();
    return { ...actual, openContextMenu: openContextMenuMock };
});

/** Build a synthetic file entry, overridable per test. */
function makeEntry(overrides: Partial<FileEntry> & { name: string; path: string }): FileEntry {
    return {
        kind: 'file',
        size: 10,
        recursive_size: null,
        modified_epoch_seconds: 0,
        owner: 'user',
        permissions: 'rw-r--r--',
        permission_class: 'normal',
        symlink_target: null,
        content_kind: 'other',
        ...overrides,
    };
}

const HOME = '/home/user';

/**
 * A minimal but complete fake `FilesIpc`. `list` resolves the given entries for
 * any path; the rest are inert resolved stubs sufficient to mount the App.
 */
function createFakeIpc(entries: FileEntry[]): FilesIpc {
    return {
        // Path-aware so the lazily-loaded sidebar tree cannot recurse: only the
        // root and Home resolve to the entries; every deeper path is empty. The
        // real daemon never lists a directory as a child of itself.
        list: vi.fn((path: string) =>
            Promise.resolve(path === '/' || path === HOME ? entries : []),
        ),
        places: vi.fn(() => Promise.resolve({ pins: [{ label: 'Home', path: HOME }], drives: [] })),
        pin: vi.fn(() => Promise.resolve([])),
        unpin: vi.fn(() => Promise.resolve([])),
        operation: vi.fn(() => Promise.resolve()),
        open: vi.fn(() => Promise.resolve()),
        openWith: vi.fn(() => Promise.resolve()),
        applications: vi.fn(() => Promise.resolve([])),
        openTerminal: vi.fn(() => Promise.resolve()),
        preview: vi.fn(() => Promise.resolve({ kind: 'none', data: '' })),
        search: vi.fn(() => Promise.resolve([])),
        watch: vi.fn(() => Promise.resolve()),
        unwatch: vi.fn(() => Promise.resolve()),
        sizes: vi.fn(() => Promise.resolve()),
        cancelSizes: vi.fn(() => Promise.resolve()),
        getPreferences: vi.fn(() => Promise.resolve({ show_hidden: true, pinned_actions: [] })),
        setPreferences: vi.fn(() => Promise.resolve()),
        subscribeFilesEvents: vi.fn(() => () => {}),
        close: vi.fn(() => {}),
    };
}

beforeEach(() => {
    openContextMenuMock.mockClear();
});

describe('App dual panes', () => {
    it('switches the active pane on Tab in dual mode', async () => {
        const ipc = createFakeIpc([makeEntry({ name: 'alpha', path: `${HOME}/alpha` })]);
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            const panes = container.querySelectorAll('.pane');
            expect(panes.length).toBe(2);
        });

        const panes = () => Array.from(container.querySelectorAll('.pane'));
        // Pane 0 active initially: pane 1 is the dimmed one.
        expect(panes()[0].classList.contains('inactive-pane')).toBe(false);
        expect(panes()[1].classList.contains('inactive-pane')).toBe(true);

        await fireEvent.keyDown(window, { key: 'Tab' });

        expect(panes()[0].classList.contains('inactive-pane')).toBe(true);
        expect(panes()[1].classList.contains('inactive-pane')).toBe(false);
    });
});

describe('App type-to-filter', () => {
    function filterEntries(): FileEntry[] {
        return [
            makeEntry({ name: 'docs', path: `${HOME}/docs`, kind: 'directory' }),
            makeEntry({ name: 'downloads', path: `${HOME}/downloads`, kind: 'directory' }),
            makeEntry({ name: 'music', path: `${HOME}/music`, kind: 'directory' }),
            makeEntry({ name: 'notes.txt', path: `${HOME}/notes.txt` }),
        ];
    }

    // The filter applies only to the active pane, and the same data-path also
    // appears in the sidebar tree and the inactive pane; scope row lookups to
    // the active pane's rows (`.frow`) so assertions are unambiguous.
    function activeRow(container: HTMLElement, path: string): Element | null {
        const active = container.querySelector('.pane:not(.inactive-pane)');
        return active?.querySelector(`.frow[data-path="${path}"]`) ?? null;
    }

    it('bare typing appends to the active filter and narrows the list live', async () => {
        const ipc = createFakeIpc(filterEntries());
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
        });

        await fireEvent.keyDown(window, { key: 'd' });
        await fireEvent.keyDown(window, { key: 'o' });

        // The toolbar filter field is the indicator: it shows the typed text.
        const filterInput = container.querySelector('.filter-input') as HTMLInputElement;
        expect(filterInput.value).toBe('do');

        // The active list is narrowed to names containing "do".
        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/docs`)).not.toBeNull();
            expect(activeRow(container, `${HOME}/downloads`)).not.toBeNull();
            expect(activeRow(container, `${HOME}/music`)).toBeNull();
            expect(activeRow(container, `${HOME}/notes.txt`)).toBeNull();
        });
    });

    it('Backspace shortens a non-empty filter instead of navigating up', async () => {
        const ipc = createFakeIpc(filterEntries());
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        await fireEvent.keyDown(window, { key: 'd' });
        await fireEvent.keyDown(window, { key: 'o' });
        await fireEvent.keyDown(window, { key: 'Backspace' });

        const filterInput = container.querySelector('.filter-input') as HTMLInputElement;
        await vi.waitFor(() => {
            expect(filterInput.value).toBe('d');
        });
        // It must not have navigated to the parent directory. Navigating up
        // resets the pane's filter, so a still-populated 'd' filter already
        // proves it did not; assert the active pane path is unchanged too.
        const activePath = container.querySelector('.pane:not(.inactive-pane) .pane-path');
        expect(activePath?.textContent).toBe(HOME);
    });

    it('Escape clears a non-empty filter and keeps the list intact', async () => {
        const ipc = createFakeIpc(filterEntries());
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
        });

        await fireEvent.keyDown(window, { key: 'd' });
        await fireEvent.keyDown(window, { key: 'o' });

        const filterInput = container.querySelector('.filter-input') as HTMLInputElement;
        await vi.waitFor(() => {
            expect(filterInput.value).toBe('do');
        });

        await fireEvent.keyDown(window, { key: 'Escape' });

        await vi.waitFor(() => {
            expect(filterInput.value).toBe('');
            // Every entry is visible again once the filter clears.
            expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
        });
    });

    it('typing directly into the filter input still filters the list', async () => {
        const ipc = createFakeIpc(filterEntries());
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
        });

        const filterInput = container.querySelector('.filter-input') as HTMLInputElement;
        await fireEvent.input(filterInput, { target: { value: 'mu' } });

        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
            expect(activeRow(container, `${HOME}/docs`)).toBeNull();
        });
    });
});

describe('App navigation', () => {
    it('navigates into a directory on double-click', async () => {
        const entries = [
            makeEntry({ name: 'docs', path: `${HOME}/docs`, kind: 'directory' }),
        ];
        const ipc = createFakeIpc(entries);
        const { container } = render(App, { props: { ipc } });

        // Wait for the panes to be re-seeded to Home so the queried row is the
        // stable node and not a detached one from the initial root render.
        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        const row = container.querySelector(`.pane [data-path="${HOME}/docs"]`) as HTMLElement;
        expect(row).not.toBeNull();
        await fireEvent.dblClick(row);

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(`${HOME}/docs`);
        });
    });
});

describe('App drive refresh debounce', () => {
    it('refetches places once after a burst of file events, after the debounce', async () => {
        const ipc = createFakeIpc([]);
        let handler: ((event: FilesEvent) => void) | null = null;
        ipc.subscribeFilesEvents = vi.fn((callback) => {
            handler = callback;
            return () => {};
        });
        render(App, { props: { ipc } });

        // Let startup settle: places is fetched once and the event handler is
        // registered. Use real timers for this async settling.
        await vi.waitFor(() => expect(handler).not.toBeNull());
        await vi.waitFor(() => expect(ipc.places).toHaveBeenCalledTimes(1));
        (ipc.places as ReturnType<typeof vi.fn>).mockClear();

        // Switch to fake timers so the debounce window is deterministic.
        vi.useFakeTimers();
        try {
            const notify = handler as unknown as (event: FilesEvent) => void;
            notify({ event: 'changed', path: '/somewhere' });
            notify({ event: 'operation_complete', operation: { kind: 'delete', paths: ['/x'] } });
            notify({ event: 'changed', path: '/elsewhere' });

            // The burst has not refetched yet; the debounce is still pending.
            expect(ipc.places).not.toHaveBeenCalled();

            await vi.advanceTimersByTimeAsync(1000);

            // Exactly one refetch collapses the whole burst.
            expect(ipc.places).toHaveBeenCalledTimes(1);
        } finally {
            vi.useRealTimers();
        }
    });
});

describe('App hidden-files preference', () => {
    it('hides dotfiles on startup when getPreferences resolves show_hidden false', async () => {
        const entries = [
            makeEntry({ name: '.hidden', path: `${HOME}/.hidden` }),
            makeEntry({ name: 'visible', path: `${HOME}/visible` }),
        ];
        const ipc = createFakeIpc(entries);
        ipc.getPreferences = vi.fn(() => Promise.resolve({ show_hidden: false, pinned_actions: [] }));
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(container.querySelector(`.pane [data-path="${HOME}/visible"]`)).not.toBeNull();
            expect(container.querySelector(`.pane [data-path="${HOME}/.hidden"]`)).toBeNull();
        });
    });
});

describe('App pinned actions', () => {
    it('writes the complete preferences object, preserving pinned_actions, when toggling hidden', async () => {
        const ipc = createFakeIpc([makeEntry({ name: 'alpha', path: `${HOME}/alpha` })]);
        ipc.getPreferences = vi.fn(() =>
            Promise.resolve({
                show_hidden: true,
                pinned_actions: [{ desktop_id: 'firefox.desktop', label: 'Open with Firefox' }],
            }),
        );
        render(App, { props: { ipc } });

        // Wait for the app to mount and load the home directory, which also means
        // the getPreferences result (including pinned_actions) has been applied.
        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        await fireEvent.keyDown(window, { key: 'h', ctrlKey: true });

        await vi.waitFor(() => {
            expect(ipc.setPreferences).toHaveBeenCalledWith(
                expect.objectContaining({
                    show_hidden: false,
                    pinned_actions: [{ desktop_id: 'firefox.desktop', label: 'Open with Firefox' }],
                }),
            );
        });
    });
});

describe('App keyboard shortcuts cheat sheet', () => {
    it('pressing ? opens the shortcuts cheat sheet', async () => {
        const ipc = createFakeIpc([]);
        const { container } = render(App, { props: { ipc } });

        await fireEvent.keyDown(window, { key: '?', shiftKey: true });

        await vi.waitFor(() => {
            expect(container.querySelector('[data-hint-row]')).not.toBeNull();
        });
    });
});

describe('App context menu and properties modal', () => {
    it('opens the entry context menu on right-click', async () => {
        const ipc = createFakeIpc([makeEntry({ name: 'alpha', path: `${HOME}/alpha` })]);
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        const row = container.querySelector(`[data-path="${HOME}/alpha"]`) as HTMLElement;
        expect(row).not.toBeNull();
        await fireEvent.contextMenu(row);

        expect(openContextMenuMock).toHaveBeenCalled();
    });

    it('closes the properties modal on Escape', async () => {
        const entry = makeEntry({ name: 'alpha', path: `${HOME}/alpha` });
        const ipc = createFakeIpc([entry]);
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        const row = container.querySelector(`[data-path="${HOME}/alpha"]`) as HTMLElement;
        expect(row).not.toBeNull();
        await fireEvent.contextMenu(row);

        // Reach into the built menu and invoke "Properties" to open the modal.
        const items = openContextMenuMock.mock.calls.at(-1)?.[1] as MenuItem[];
        const properties = items.find((item) => item.label === 'Properties');
        expect(properties).toBeTruthy();
        properties?.onSelect?.();

        await vi.waitFor(() => {
            expect(container.querySelector('.properties-modal')).not.toBeNull();
        });

        await fireEvent.keyDown(window, { key: 'Escape' });

        await vi.waitFor(() => {
            expect(container.querySelector('.properties-modal')).toBeNull();
        });
    });
});

describe('App recursive-size requests and completion tracking', () => {
    /** Count how many times `ipc.sizes` was called for a specific path. */
    function sizesCallsFor(ipc: FilesIpc, path: string): number {
        return (ipc.sizes as ReturnType<typeof vi.fn>).mock.calls.filter(
            (call) => call[0] === path,
        ).length;
    }

    it('requests recursive sizes for a pane path on the initial load', async () => {
        const ipc = createFakeIpc([makeEntry({ name: 'alpha', path: `${HOME}/alpha` })]);
        render(App, { props: { ipc } });

        // After the panes re-seed to Home and list, sizes must have been asked
        // for the Home path at least once (both panes share it).
        await vi.waitFor(() => {
            expect(sizesCallsFor(ipc, HOME)).toBeGreaterThanOrEqual(1);
        });
    });

    /** Count how many times `ipc.cancelSizes` was called for a specific path. */
    function cancelSizesCallsFor(ipc: FilesIpc, path: string): number {
        return (ipc.cancelSizes as ReturnType<typeof vi.fn>).mock.calls.filter(
            (call) => call[0] === path,
        ).length;
    }

    it('re-requests recursive sizes for the pane path after a changed reload (Defect A)', async () => {
        const ipc = createFakeIpc([
            makeEntry({ name: 'docs', path: `${HOME}/docs`, kind: 'directory' }),
        ]);
        let handler: ((event: FilesEvent) => void) | null = null;
        ipc.subscribeFilesEvents = vi.fn((callback) => {
            handler = callback;
            return () => {};
        });
        render(App, { props: { ipc } });

        // Settle: the event handler is registered and the initial size request
        // for Home has already fired.
        await vi.waitFor(() => expect(handler).not.toBeNull());
        await vi.waitFor(() => {
            expect(sizesCallsFor(ipc, HOME)).toBeGreaterThanOrEqual(1);
        });
        const before = sizesCallsFor(ipc, HOME);
        const cancelsBefore = cancelSizesCallsFor(ipc, HOME);

        // A `changed` event on the current pane's path reloads it AND must
        // re-request its sizes; the count for Home must strictly increase.
        const notify = handler as unknown as (event: FilesEvent) => void;
        notify({ event: 'changed', path: HOME });

        await vi.waitFor(() => {
            expect(sizesCallsFor(ipc, HOME)).toBeGreaterThan(before);
        });
        // The re-request must be preceded by a matching cancel for the same
        // path so the backend reference count stays balanced: the navigation
        // effect already left one sizes() outstanding for Home, so a second
        // sizes() from the reload without a paired cancelSizes() would inflate
        // the count and leave a walk running for a folder the user has left.
        await vi.waitFor(() => {
            expect(cancelSizesCallsFor(ipc, HOME)).toBeGreaterThan(cancelsBefore);
        });
    });

    it('preserves a known child recursive size across a changed reload', async () => {
        // The directory row already has a recursive size of 12345 bytes,
        // rendered as "12.1 KB". A reload replaces the entries with fresh
        // objects whose recursive_size is null, so without preservation the
        // size text would blank out. It must survive the reload.
        // The real daemon returns FRESH entry objects on every `list`, with
        // recursive_size null until sizing re-computes it. Model that here so
        // the test actually exercises preservation: the first list carries the
        // known size, every later list nulls it out.
        const ipc = createFakeIpc([]);
        // The daemon nulls recursive_size on a fresh list until sizing recomputes
        // it. `sizeCleared` flips to model the post-`changed` reload: initial
        // loads carry the known size, the reload returns null.
        let sizeCleared = false;
        ipc.list = vi.fn((path: string) => {
            if (path !== HOME) {
                return Promise.resolve([]);
            }
            return Promise.resolve([
                makeEntry({
                    name: 'docs',
                    path: `${HOME}/docs`,
                    kind: 'directory',
                    recursive_size: sizeCleared ? null : 12345,
                }),
            ]);
        });
        let handler: ((event: FilesEvent) => void) | null = null;
        ipc.subscribeFilesEvents = vi.fn((callback) => {
            handler = callback;
            return () => {};
        });
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => expect(handler).not.toBeNull());
        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        function docsSizeText(): string | null {
            const active = container.querySelector('.pane:not(.inactive-pane)');
            const row = active?.querySelector(`.frow[data-path="${HOME}/docs"]`);
            return row?.querySelector('.szval')?.textContent ?? null;
        }

        await vi.waitFor(() => {
            expect(docsSizeText()).toBe('12.1 KB');
        });

        function homeListCalls(): number {
            return (ipc.list as ReturnType<typeof vi.fn>).mock.calls.filter(
                (call) => call[0] === HOME,
            ).length;
        }
        const listsBefore = homeListCalls();

        // From now on a fresh list nulls the recursive size; only the App's
        // preservation of the previously-known value can keep it visible.
        sizeCleared = true;
        const notify = handler as unknown as (event: FilesEvent) => void;
        notify({ event: 'changed', path: HOME });

        // Wait until the reload has actually re-listed Home and the new
        // null-sized entries have been applied to the pane.
        await vi.waitFor(() => {
            expect(homeListCalls()).toBeGreaterThan(listsBefore);
        });
        // Give the reassigned entries a chance to render.
        await new Promise((resolve) => setTimeout(resolve, 0));

        // After the reload the known recursive size is re-applied, so the size
        // text stays "12.1 KB" rather than falling back to the on-disk "10 B".
        expect(docsSizeText()).toBe('12.1 KB');
    });
});

describe('App size-event batching', () => {
    it('buffers size events and applies them together on the debounced flush', async () => {
        // Two directories, each with an unknown recursive size (renders as the
        // on-disk "10 B") and each still in the sizing set from the initial
        // markSizing. Their `size` events must NOT apply one-by-one; they are
        // buffered and applied together so the size sort runs once per batch.
        const entries = [
            makeEntry({ name: 'alpha', path: `${HOME}/alpha`, kind: 'directory' }),
            makeEntry({ name: 'beta', path: `${HOME}/beta`, kind: 'directory' }),
        ];
        const ipc = createFakeIpc(entries);
        let handler: ((event: FilesEvent) => void) | null = null;
        ipc.subscribeFilesEvents = vi.fn((callback) => {
            handler = callback;
            return () => {};
        });
        const { container } = render(App, { props: { ipc } });

        // Let startup settle with real timers: the handler registers and the
        // initial listing renders both rows in the active pane.
        await vi.waitFor(() => expect(handler).not.toBeNull());

        function sizeText(path: string): string | null {
            const active = container.querySelector('.pane:not(.inactive-pane)');
            const row = active?.querySelector(`.frow[data-path="${path}"]`);
            return row?.querySelector('.szval')?.textContent?.trim() ?? null;
        }
        function hasCalculatingDot(path: string): boolean {
            const active = container.querySelector('.pane:not(.inactive-pane)');
            const row = active?.querySelector(`.frow[data-path="${path}"]`);
            return row?.querySelector('.size-calculating') !== null;
        }

        // Both rows render with the on-disk fallback size and a calculating dot
        // (they are in the sizing set awaiting their recursive total).
        await vi.waitFor(() => {
            expect(sizeText(`${HOME}/alpha`)).toBe('10 B');
            expect(sizeText(`${HOME}/beta`)).toBe('10 B');
            expect(hasCalculatingDot(`${HOME}/alpha`)).toBe(true);
            expect(hasCalculatingDot(`${HOME}/beta`)).toBe(true);
        });

        // Switch to fake timers so the flush's setTimeout is deterministic. The
        // buffer flush runs on a macrotask (setTimeout); Svelte commits DOM
        // updates on a microtask. Awaiting a microtask (Promise.resolve) below
        // lets Svelte render any ALREADY-applied state WITHOUT running the
        // macrotask flush — so if the events were applied synchronously the DOM
        // would already show the new sizes here, failing the assertion.
        vi.useFakeTimers();
        try {
            const notify = handler as unknown as (event: FilesEvent) => void;
            notify({ event: 'size', path: `${HOME}/alpha`, bytes: 12345, complete: true });
            notify({ event: 'size', path: `${HOME}/beta`, bytes: 2048, complete: true });

            // Flush microtasks (Svelte's render commit) but NOT the macrotask
            // buffer flush. The events are buffered, not applied synchronously:
            // the rendered sizes are unchanged and the calculating dots remain.
            await Promise.resolve();
            await Promise.resolve();
            expect(sizeText(`${HOME}/alpha`)).toBe('10 B');
            expect(sizeText(`${HOME}/beta`)).toBe('10 B');
            expect(hasCalculatingDot(`${HOME}/alpha`)).toBe(true);
            expect(hasCalculatingDot(`${HOME}/beta`)).toBe(true);

            // Advancing past the debounce runs the flush once, applying BOTH
            // buffered events together and clearing their sizing dots.
            await vi.advanceTimersByTimeAsync(0);

            expect(sizeText(`${HOME}/alpha`)).toBe('12.1 KB');
            expect(sizeText(`${HOME}/beta`)).toBe('2 KB');
            expect(hasCalculatingDot(`${HOME}/alpha`)).toBe(false);
            expect(hasCalculatingDot(`${HOME}/beta`)).toBe(false);
        } finally {
            vi.useRealTimers();
        }
    });
});

describe('App path argument (window.__quantum_args)', () => {
    const TARGET = '/usr/share/icons';

    afterEach(() => {
        delete (window as any).__quantum_args;
    });

    it('opens to the args.path directory when it is a valid, listable directory', async () => {
        (window as any).__quantum_args = { path: TARGET };
        const ipc = createFakeIpc([]);
        // Allow the target path to be listed successfully (it is a directory).
        ipc.list = vi.fn((path: string) =>
            Promise.resolve(
                path === TARGET
                    ? [makeEntry({ name: 'hicolor', path: `${TARGET}/hicolor`, kind: 'directory' })]
                    : [],
            ),
        );
        const { container } = render(App, { props: { ipc } });

        // The pane navigates to the args path, so list is called with it.
        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(TARGET);
        });

        // The active pane path label shows the target directory.
        const activePath = container.querySelector('.pane:not(.inactive-pane) .pane-path');
        await vi.waitFor(() => {
            expect(activePath?.textContent).toBe(TARGET);
        });
    });

    it('falls back to home when args.path is not a valid directory', async () => {
        (window as any).__quantum_args = { path: '/nonexistent/path' };
        const consoleWarn = vi.spyOn(console, 'warn').mockImplementation(() => {});
        const ipc = createFakeIpc([makeEntry({ name: 'alpha', path: `${HOME}/alpha` })]);
        // The target path rejects (does not exist); Home still resolves.
        const originalList = ipc.list as ReturnType<typeof vi.fn>;
        ipc.list = vi.fn((path: string) => {
            if (path === '/nonexistent/path') {
                return Promise.reject(new Error('No such directory'));
            }
            return originalList(path);
        });
        const { container } = render(App, { props: { ipc } });

        // It must fall back to the home directory.
        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });
        const activePath = container.querySelector('.pane:not(.inactive-pane) .pane-path');
        await vi.waitFor(() => {
            expect(activePath?.textContent).toBe(HOME);
        });

        // A console warning is emitted for the invalid path.
        expect(consoleWarn).toHaveBeenCalledWith(
            expect.stringContaining('/nonexistent/path'),
        );
        consoleWarn.mockRestore();
    });

    it('opens to home when no args are provided (existing behavior)', async () => {
        // No __quantum_args set.
        const ipc = createFakeIpc([makeEntry({ name: 'alpha', path: `${HOME}/alpha` })]);
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });
        const activePath = container.querySelector('.pane:not(.inactive-pane) .pane-path');
        await vi.waitFor(() => {
            expect(activePath?.textContent).toBe(HOME);
        });
    });
});
