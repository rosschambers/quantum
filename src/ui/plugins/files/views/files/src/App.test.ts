import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import type { FileEntry, MenuItem } from '@quantum/client';
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
        getPreferences: vi.fn(() => Promise.resolve({ show_hidden: true })),
        setPreferences: vi.fn(() => Promise.resolve()),
        subscribeFilesEvents: vi.fn(() => () => {}),
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

describe('App type-ahead', () => {
    it('selects the first entry whose name starts with the typed buffer', async () => {
        const entries = [
            makeEntry({ name: 'apple', path: `${HOME}/apple` }),
            makeEntry({ name: 'banana', path: `${HOME}/banana` }),
            makeEntry({ name: 'cherry', path: `${HOME}/cherry` }),
        ];
        const ipc = createFakeIpc(entries);
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(container.querySelector(`[data-path="${HOME}/banana"]`)).not.toBeNull();
        });

        await fireEvent.keyDown(window, { key: 'b' });

        const selected = container.querySelector('.frow.sel');
        expect(selected).not.toBeNull();
        expect(selected?.textContent).toContain('banana');
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

describe('App hidden-files preference', () => {
    it('hides dotfiles on startup when getPreferences resolves show_hidden false', async () => {
        const entries = [
            makeEntry({ name: '.hidden', path: `${HOME}/.hidden` }),
            makeEntry({ name: 'visible', path: `${HOME}/visible` }),
        ];
        const ipc = createFakeIpc(entries);
        ipc.getPreferences = vi.fn(() => Promise.resolve({ show_hidden: false }));
        const { container } = render(App, { props: { ipc } });

        await vi.waitFor(() => {
            expect(container.querySelector(`.pane [data-path="${HOME}/visible"]`)).not.toBeNull();
            expect(container.querySelector(`.pane [data-path="${HOME}/.hidden"]`)).toBeNull();
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
