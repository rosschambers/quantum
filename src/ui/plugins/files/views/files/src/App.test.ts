import './testSvelteRuntime';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import type { FileEntry, FilesEvent, MenuItem } from '@quantum/client';
import App from './App.svelte';
import { tick } from 'svelte';
import { PaneState } from './lib/paneState.svelte';
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

    it('typing selects the first match so Enter can open it', async () => {
        const ipc = createFakeIpc(filterEntries());
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
        });

        await fireEvent.keyDown(window, { key: 'd' });
        await fireEvent.keyDown(window, { key: 'o' });

        // The narrowed list is [docs, downloads] (folders first, name order);
        // the first match must carry the selection class.
        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/docs`)?.classList.contains('sel')).toBe(true);
        });
    });

    it('Backspace re-syncs the selection to the first match of the shortened filter', async () => {
        const ipc = createFakeIpc(filterEntries());
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
        });

        // "dow" narrows the list to downloads only (docs does not contain "dow").
        await fireEvent.keyDown(window, { key: 'd' });
        await fireEvent.keyDown(window, { key: 'o' });
        await fireEvent.keyDown(window, { key: 'w' });
        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/downloads`)?.classList.contains('sel')).toBe(true);
        });

        // Backspace shortens the filter to "do", which widens the list back to
        // [docs, downloads] (folders first, name order); the selection must
        // re-sync to the new first match, docs.
        await fireEvent.keyDown(window, { key: 'Backspace' });
        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/docs`)?.classList.contains('sel')).toBe(true);
        });
    });

    it('clearing the filter clears the selection', async () => {
        const ipc = createFakeIpc(filterEntries());
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
        });

        await fireEvent.keyDown(window, { key: 'd' });
        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/docs`)?.classList.contains('sel')).toBe(true);
        });

        await fireEvent.keyDown(window, { key: 'Backspace' }); // filter empty -> selection cleared
        await vi.waitFor(() => {
            expect(container.querySelector('.pane:not(.inactive-pane) .frow.sel')).toBeNull();
        });
    });

    it('Enter opens the first match after a stale cursor from a prior selection', async () => {
        const ipc = createFakeIpc(filterEntries());
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
        });

        // Click the last row (notes.txt) so the cursor sits at index 3.
        await fireEvent.click(activeRow(container, `${HOME}/notes.txt`)!);

        // Type a prefix matching exactly one entry: the selection sync (Task 1)
        // moves the highlight to `docs`, so Enter must open `docs`.
        await fireEvent.keyDown(window, { key: 'd' });
        await fireEvent.keyDown(window, { key: 'o' });
        await fireEvent.keyDown(window, { key: 'w' });
        await vi.waitFor(() => {
            expect(activeRow(container, `${HOME}/downloads`)?.classList.contains('sel')).toBe(true);
        });

        await fireEvent.keyDown(window, { key: 'Enter' });

        // Navigating into a directory lists it (see the double-click test pattern).
        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(`${HOME}/downloads`);
        });
    });
});

describe('App navigation', () => {
    it.each([
        { rejectOld: false, oldFirst: false },
        { rejectOld: true, oldFirst: false },
        { rejectOld: false, oldFirst: true },
        { rejectOld: true, oldFirst: true },
    ])('guards rows, loading and errors after navigating to root: %j', async ({ rejectOld, oldFirst }) => {
        const navigation = vi.spyOn(PaneState.prototype, 'navigate');
        const ipc = createFakeIpc([makeEntry({ name: 'old', path: `${HOME}/old` })]);
        const { container } = render(App, { props: { ipc } });
        await vi.waitFor(() => expect(ipc.sizes).toHaveBeenCalledWith(HOME));
        const pane = navigation.mock.contexts[0];
        navigation.mockRestore();
        let resolveOld!: (entries: FileEntry[]) => void;
        let rejectOldRequest!: (error: Error) => void;
        let resolveRoot!: (entries: FileEntry[]) => void;
        ipc.list = vi.fn((path) => {
            if (path === HOME) return new Promise<FileEntry[]>((resolve, reject) => {
                resolveOld = resolve;
                rejectOldRequest = reject;
            });
            if (path === '/') return new Promise<FileEntry[]>((resolve) => { resolveRoot = resolve; });
            return Promise.resolve([]);
        });
        await fireEvent.keyDown(window, { key: 'F5' });
        await fireEvent.keyDown(window, { key: 'ArrowUp', altKey: true });
        await fireEvent.keyDown(window, { key: 'ArrowUp', altKey: true });
        expect(pane.path).toBe('/');
        expect(pane.loading).toBe(true);
        const toastText = container.querySelector('#toasts')?.textContent;
        function settleOld(): void {
            if (rejectOld) rejectOldRequest(new Error('Old directory unavailable'));
            else resolveOld([makeEntry({ name: 'stale', path: `${HOME}/stale` })]);
        }
        if (oldFirst) {
            settleOld();
            await tick();
            expect(pane.loading).toBe(true);
        }
        resolveRoot([makeEntry({ name: 'current', path: '/current' })]);
        await tick();
        if (!oldFirst) {
            settleOld();
            await tick();
        }
        expect(pane.loading).toBe(false);
        expect(pane.entries.map((entry) => entry.path)).toEqual(['/current']);
        await vi.waitFor(() => expect(container.querySelector('.pane:not(.inactive-pane) .frow[data-path="/current"]')).not.toBeNull());
        expect(container.querySelector('#toasts')?.textContent).toBe(toastText);
    });

    it('keeps the newest same-path reload when an older generation resolves last', async () => {
        const ipc = createFakeIpc([]);
        const { container } = render(App, { props: { ipc } });
        await vi.waitFor(() => expect(ipc.sizes).toHaveBeenCalledWith(HOME));
        const held: Array<(entries: FileEntry[]) => void> = [];
        ipc.list = vi.fn(() => new Promise<FileEntry[]>((resolve) => held.push(resolve)));
        await fireEvent.keyDown(window, { key: 'F5' });
        await fireEvent.keyDown(window, { key: 'F5' });
        expect(held).toHaveLength(2);
        held[1]([makeEntry({ name: 'current', path: `${HOME}/current` })]);
        await tick();
        held[0]([makeEntry({ name: 'stale', path: `${HOME}/stale` })]);
        await tick();
        expect(container.querySelector('.pane:not(.inactive-pane) .frow[data-path="/home/user/current"]')).not.toBeNull();
        expect(container.querySelector('.pane:not(.inactive-pane) .frow[data-path="/home/user/stale"]')).toBeNull();
    });

    it('Alt+Up navigates to the parent rather than moving the selection cursor', async () => {
        const ipc = createFakeIpc([makeEntry({ name: 'alpha', path: `${HOME}/alpha` })]);
        const { container } = render(App, { props: { ipc } });
        await vi.waitFor(() => expect(container.querySelector('.pane-path')?.textContent).toBe(HOME));
        await fireEvent.keyDown(window, { key: 'ArrowUp', altKey: true });
        expect(container.querySelector('.pane-path')?.textContent).toBe('/home');
    });

    it('ignores a held reload after navigation, including its size-request continuation', async () => {
        const ipc = createFakeIpc([makeEntry({ name: 'old', path: `${HOME}/old` })]);
        let notify!: (event: FilesEvent) => void;
        ipc.subscribeFilesEvents = vi.fn((callback) => { notify = callback; return () => {}; });
        const { container } = render(App, { props: { ipc } });
        await vi.waitFor(() => expect(ipc.sizes).toHaveBeenCalledWith(HOME));
        const held: Array<(entries: FileEntry[]) => void> = [];
        ipc.list = vi.fn((path) => path === HOME
            ? new Promise<FileEntry[]>((resolve) => held.push(resolve))
            : Promise.resolve([makeEntry({ name: 'new', path: '/home/new' })]));
        notify({ event: 'changed', path: HOME });
        await fireEvent.keyDown(window, { key: 'Backspace' });
        await vi.waitFor(() => expect(container.querySelector('.pane:not(.inactive-pane) .frow[data-path="/home/new"]')).not.toBeNull());
        const sizeCalls = vi.mocked(ipc.sizes).mock.calls.filter(([path]) => path === '/home').length;
        for (const resolve of held) resolve([makeEntry({ name: 'stale', path: `${HOME}/stale` })]);
        await new Promise((resolve) => setTimeout(resolve, 20));
        expect(container.querySelector('.pane:not(.inactive-pane) .frow[data-path="/home/new"]')).not.toBeNull();
        expect(vi.mocked(ipc.sizes).mock.calls.filter(([path]) => path === '/home').length).toBe(sizeCalls);
        expect(container.querySelector('.inactive-pane .frow[data-path="/home/user/stale"]')).not.toBeNull();
    });
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

describe('App search shortcuts', () => {
    it('Ctrl+F focuses the toolbar filter input', async () => {
        const ipc = createFakeIpc([]);
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const filterInput = container.querySelector('.filter-input') as HTMLInputElement;
        expect(document.activeElement).toBe(filterInput);
    });

    it('Ctrl+F focuses the filter input even when focus is already inside it', async () => {
        const ipc = createFakeIpc([]);
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        const filterInput = container.querySelector('.filter-input') as HTMLInputElement;
        filterInput.focus();
        expect(document.activeElement).toBe(filterInput);

        await fireEvent.keyDown(filterInput, { key: 'f', ctrlKey: true });

        expect(document.activeElement).toBe(filterInput);
    });

    it('Ctrl+A inside the filter input does not dispatch select-all', async () => {
        const ipc = createFakeIpc([makeEntry({ name: 'alpha', path: `${HOME}/alpha` })]);
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        const filterInput = container.querySelector('.filter-input') as HTMLInputElement;
        filterInput.focus();

        await fireEvent.keyDown(filterInput, { key: 'a', ctrlKey: true });

        const activeRow = container.querySelector('.pane:not(.inactive-pane) .frow.selected');
        expect(activeRow).toBeNull();
    });

    it('Ctrl+Shift+F toggles deep search on and off and focuses the filter input', async () => {
        const ipc = createFakeIpc([]);
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        const deepButton = container.querySelector('.deep') as HTMLButtonElement;
        expect(deepButton.classList.contains('on')).toBe(false);

        await fireEvent.keyDown(window, { key: 'F', ctrlKey: true, shiftKey: true });

        expect(deepButton.classList.contains('on')).toBe(true);
        const filterInput = container.querySelector('.filter-input') as HTMLInputElement;
        expect(document.activeElement).toBe(filterInput);

        filterInput.blur();
        await fireEvent.keyDown(window, { key: 'F', ctrlKey: true, shiftKey: true });

        expect(deepButton.classList.contains('on')).toBe(false);
        expect(document.activeElement).toBe(filterInput);
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
    it('visits each pane entry once per batch rather than searching for every size event', async () => {
        const navigation = vi.spyOn(PaneState.prototype, 'navigate');
        const entries = Array.from({ length: 1000 }, (_, index) => makeEntry({ name: `entry-${index}`, path: `${HOME}/entry-${index}` }));
        const ipc = createFakeIpc(entries);
        let notify!: (event: FilesEvent) => void;
        ipc.subscribeFilesEvents = vi.fn((callback) => { notify = callback; return () => {}; });
        const { container } = render(App, { props: { ipc } });
        await vi.waitFor(() => expect(ipc.sizes).toHaveBeenCalledWith(HOME));
        const panes = [...navigation.mock.contexts];
        navigation.mockRestore();
        const unchangedEntries = panes.map((pane) => pane.entries[0]);
        await fireEvent.click(container.querySelector('.pane .c-size')!);
        const listItems = vi.spyOn(PaneState.prototype, 'listItems');
        const find = vi.spyOn(Array.prototype, 'find');
        vi.useFakeTimers();
        try {
            for (let index = 800; index < 1000; index += 1) {
                notify({ event: 'size', path: `${HOME}/entry-${index}`, bytes: index, complete: true });
            }
            notify({ event: 'size', path: `${HOME}/entry-800`, bytes: 2048, complete: false });
            await vi.advanceTimersByTimeAsync(0);
            expect(find.mock.calls.length).toBeLessThan(10);
            expect(listItems.mock.contexts.filter((pane) => pane === panes[0])).toHaveLength(1);
            for (const [index, pane] of panes.entries()) {
                expect(pane.entries[0]).toBe(unchangedEntries[index]);
                expect(pane.entries[0].recursive_size).toBeNull();
                expect(pane.entries[800].recursive_size).toBe(2048);
                expect(pane.entries[999].recursive_size).toBe(999);
                expect(pane.sizing.has(`${HOME}/entry-800`)).toBe(true);
                expect(pane.sizing.has(`${HOME}/entry-999`)).toBe(false);
            }
        } finally {
            listItems.mockRestore();
            find.mockRestore();
            vi.useRealTimers();
        }
    });
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

describe('App deep-search grouped headers', () => {
    it.each(['root group', 'current breadcrumb'])('reloads the directory when leaving search through the %s', async (target) => {
        const ipc = createFakeIpc([
            makeEntry({ name: 'ordinary.txt', path: `${HOME}/ordinary.txt` }),
            makeEntry({ name: 'match.txt', path: `${HOME}/match.txt` }),
        ]);
        ipc.search = vi.fn(() => Promise.resolve([
            makeEntry({ name: 'match.txt', path: `${HOME}/match.txt` }),
            makeEntry({ name: 'nested-match.txt', path: `${HOME}/sub/nested-match.txt` }),
        ]));
        const { container } = render(App, { props: { ipc } });
        const activePane = container.querySelector('.pane:not(.inactive-pane)')!;
        await vi.waitFor(() => expect(ipc.sizes).toHaveBeenCalledWith(HOME));
        await fireEvent.keyDown(window, { key: 'F', ctrlKey: true, shiftKey: true });
        await fireEvent.keyDown(window, { key: 'm' });
        await vi.waitFor(() => {
            expect(activePane.querySelectorAll('.group-header')).toHaveLength(2);
            expect(activePane.querySelector('.frow.sel')).not.toBeNull();
        });
        expect(activePane.querySelector('.frow[data-path="/home/user/ordinary.txt"]')).toBeNull();
        const listingCalls = vi.mocked(ipc.list).mock.calls.length;
        const watchCalls = vi.mocked(ipc.watch).mock.calls.length;
        const sizeCalls = vi.mocked(ipc.sizes).mock.calls.length;
        if (target === 'root group') {
            const header = activePane.querySelector('.group-header')!;
            expect(header.querySelector('.group-label')?.textContent).toBe('.');
            await fireEvent.click(header);
        } else {
            await fireEvent.click(container.querySelector('.crumbs .seg.last')!);
        }
        await vi.waitFor(() => {
            // Breadcrumb navigation retains its existing validation listing.
            expect(vi.mocked(ipc.list).mock.calls.length).toBe(listingCalls + (target === 'root group' ? 1 : 2));
            expect(activePane.querySelector('.frow[data-path="/home/user/ordinary.txt"]')).not.toBeNull();
            expect(vi.mocked(ipc.sizes).mock.calls.length).toBe(sizeCalls + 1);
        });
        expect(activePane.querySelectorAll('.frow')).toHaveLength(2);
        expect(activePane.querySelector('.frow[data-path="/home/user/sub/nested-match.txt"]')).toBeNull();
        expect(activePane.querySelector('.frow.sel')).toBeNull();
        expect(activePane.querySelector('.group-header')).toBeNull();
        expect(activePane.querySelector('.pane-path')?.textContent).toBe(HOME);
        expect(container.querySelector<HTMLInputElement>('.filter-input')?.value).toBe('');
        expect(container.querySelector('.deep')?.classList.contains('on')).toBe(false);
        expect(vi.mocked(ipc.watch).mock.calls.length).toBe(watchCalls + 1);
    });

    it('clicking a group header navigates there and clears filter and deep search', async () => {
        const ipc = createFakeIpc([]);
        ipc.search = vi.fn(() =>
            Promise.resolve([
                makeEntry({ name: 'root.txt', path: `${HOME}/root.txt` }),
                makeEntry({ name: 'nested.txt', path: `${HOME}/sub/nested.txt` }),
            ]),
        );
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        // Turn deep search on, then type a filter character: each keystroke
        // is live-narrowed through the same path bare-typing uses, and with
        // deep search active it re-runs `ipc.search`.
        await fireEvent.keyDown(window, { key: 'F', ctrlKey: true, shiftKey: true });
        await fireEvent.keyDown(window, { key: 't' });

        await vi.waitFor(() => {
            expect(ipc.search).toHaveBeenCalled();
        });

        // Two groups render: the search root ("."), and "sub".
        const headers = await vi.waitFor(() => {
            const found = container.querySelectorAll(
                '.pane:not(.inactive-pane) .group-header',
            );
            expect(found.length).toBe(2);
            return found;
        });
        const labels = [...headers].map((header) => header.querySelector('.group-label')?.textContent);
        expect(labels).toEqual(['.', 'sub']);

        const subHeader = headers[1] as HTMLElement;
        await fireEvent.click(subHeader);

        // Navigated to the group's absolute path.
        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(`${HOME}/sub`);
        });
        const activePath = container.querySelector('.pane:not(.inactive-pane) .pane-path');
        expect(activePath?.textContent).toBe(`${HOME}/sub`);

        // Filter and deep search are both cleared by the navigation.
        const filterInput = container.querySelector('.filter-input') as HTMLInputElement;
        expect(filterInput.value).toBe('');
        const deepButton = container.querySelector('.deep') as HTMLButtonElement;
        expect(deepButton.classList.contains('on')).toBe(false);
    });

    it('clicking a group header never selects a row', async () => {
        const ipc = createFakeIpc([]);
        ipc.search = vi.fn(() =>
            Promise.resolve([makeEntry({ name: 'root.txt', path: `${HOME}/root.txt` })]),
        );
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(ipc.list).toHaveBeenCalledWith(HOME);
        });

        await fireEvent.keyDown(window, { key: 'F', ctrlKey: true, shiftKey: true });
        await fireEvent.keyDown(window, { key: 't' });

        const header = await vi.waitFor(() => {
            const found = container.querySelector('.pane:not(.inactive-pane) .group-header');
            expect(found).not.toBeNull();
            return found as HTMLElement;
        });
        await fireEvent.click(header);

        const selectedRow = container.querySelector('.pane:not(.inactive-pane) .frow.sel');
        expect(selectedRow).toBeNull();
    });
});
