import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import type { FileEntry } from '@quantum/client';
import type { ListItem } from './paneState.svelte';
import FileList from './FileList.svelte';
import { beginDrag, endDrag } from './dragState.svelte';

/** A minimal in-memory DataTransfer stand-in; jsdom does not implement one. */
function fakeDataTransfer() {
    const store: Record<string, string> = {};
    return {
        setData: vi.fn((type: string, value: string) => {
            store[type] = value;
        }),
        getData: vi.fn((type: string) => store[type] ?? ''),
    };
}

/** Build a synthetic entry for index `i`. */
function makeEntry(index: number): FileEntry {
    return {
        name: `entry-${index}`,
        path: `/dir/entry-${index}`,
        kind: 'file',
        size: index,
        recursive_size: null,
        modified_epoch_seconds: 0,
        owner: 'user',
        permissions: 'rw-r--r--',
        permission_class: 'normal',
        symlink_target: null,
        content_kind: 'other',
    };
}

function makeEntries(count: number): FileEntry[] {
    const entries: FileEntry[] = [];
    for (let index = 0; index < count; index += 1) {
        entries.push(makeEntry(index));
    }
    return entries;
}

/** Wrap plain entries as unheaded `ListItem`s, the shape `FileList` now consumes. */
function toItems(entries: FileEntry[]): ListItem[] {
    return entries.map((entry) => ({ kind: 'entry', entry }));
}

function renderList(entries: FileEntry[], viewportHeight: number) {
    return render(FileList, {
        props: {
            items: toItems(entries),
            selection: new Set<string>(),
            maxSize: entries.length,
            viewportHeight,
            onSelect: vi.fn(),
            onOpen: vi.fn(),
            onContextMenu: vi.fn(),
        },
    });
}

describe('FileList virtualization', () => {
    it('renders far fewer than the total rows for a huge list', () => {
        // viewportHeight is injected because jsdom does not lay out or measure
        // clientHeight; see the FileList prop documentation.
        const { container } = renderList(makeEntries(50000), 600);
        const rowCount = container.querySelectorAll('.frow').length;
        // 600px viewport / 30px rows = 20 visible + 2 * 10 overscan = 40 rows.
        expect(rowCount).toBe(40);
        expect(rowCount).toBeLessThan(80);
    });

    it('reflects the full list height on the scroll sizer', () => {
        const { container } = renderList(makeEntries(50000), 600);
        const sizer = container.querySelector('.sizer') as HTMLElement;
        expect(sizer.style.height).toBe(`${50000 * 30}px`);
    });

    it('shifts which rows render when the container scrolls', async () => {
        const { container } = renderList(makeEntries(50000), 600);
        // Near the top: entry 0 is mounted, an entry deep in the list is not.
        expect(container.querySelector('[data-path="/dir/entry-0"]')).not.toBeNull();
        expect(container.querySelector('[data-path="/dir/entry-1000"]')).toBeNull();

        const scroller = container.querySelector('.list') as HTMLElement;
        scroller.scrollTop = 1000 * 30;
        await fireEvent.scroll(scroller);

        // After scrolling to index 1000, that row is mounted and the top is gone.
        expect(container.querySelector('[data-path="/dir/entry-1000"]')).not.toBeNull();
        expect(container.querySelector('[data-path="/dir/entry-0"]')).toBeNull();
    });
});

describe('FileList background drop target', () => {
    afterEach(() => endDrag());

    function renderEmptyList(path: string, onMove: (sources: string[], destination: string) => void) {
        return render(FileList, {
            props: {
                items: [] as ListItem[],
                selection: new Set<string>(),
                maxSize: 1,
                viewportHeight: 600,
                path,
                onMove,
                onSelect: vi.fn(),
                onOpen: vi.fn(),
                onContextMenu: vi.fn(),
            },
        });
    }

    it('moves a drag from the other pane into the current directory', async () => {
        const onMove = vi.fn();
        const { container } = renderEmptyList('/panes/right', onMove);
        const list = container.querySelector('.list') as HTMLElement;
        beginDrag(['/panes/left/a', '/panes/left/b']);
        const dataTransfer = fakeDataTransfer();

        await fireEvent.dragOver(list, { dataTransfer });
        expect(list.classList.contains('droptarget')).toBe(true);

        await fireEvent.drop(list, { dataTransfer });
        expect(onMove).toHaveBeenCalledTimes(1);
        expect(onMove).toHaveBeenCalledWith(['/panes/left/a', '/panes/left/b'], '/panes/right');
        expect(list.classList.contains('droptarget')).toBe(false);
    });

    it('does nothing when the current directory is one of the sources', async () => {
        const onMove = vi.fn();
        const { container } = renderEmptyList('/panes/right', onMove);
        const list = container.querySelector('.list') as HTMLElement;
        beginDrag(['/panes/right', '/panes/left/b']);
        const dataTransfer = fakeDataTransfer();

        await fireEvent.dragOver(list, { dataTransfer });
        expect(list.classList.contains('droptarget')).toBe(false);
        await fireEvent.drop(list, { dataTransfer });
        expect(onMove).not.toHaveBeenCalled();
    });

    it('moves a drop landing on the inner content, not only the thin list edge', async () => {
        const onMove = vi.fn();
        const { container } = render(FileList, {
            props: {
                items: toItems(makeEntries(3)),
                selection: new Set<string>(),
                maxSize: 3,
                viewportHeight: 600,
                path: '/panes/right',
                onMove,
                onSelect: vi.fn(),
                onOpen: vi.fn(),
                onContextMenu: vi.fn(),
            },
        });
        // The full-height sizer covers the list; a drop on it (target is a child
        // of the scroll container, not equal to it) must still move into the pane.
        const sizer = container.querySelector('.sizer') as HTMLElement;
        beginDrag(['/panes/left/a']);
        const dataTransfer = fakeDataTransfer();

        await fireEvent.dragOver(sizer, { dataTransfer });
        const list = container.querySelector('.list') as HTMLElement;
        expect(list.classList.contains('droptarget')).toBe(true);

        await fireEvent.drop(sizer, { dataTransfer });
        expect(onMove).toHaveBeenCalledTimes(1);
        expect(onMove).toHaveBeenCalledWith(['/panes/left/a'], '/panes/right');
    });

    it('routes a drop on a directory row to that row and does not also fire the list', async () => {
        const onMove = vi.fn();
        const directory: FileEntry = {
            ...makeEntry(0),
            name: 'sub',
            path: '/dir/sub',
            kind: 'directory',
        };
        const { container } = render(FileList, {
            props: {
                items: toItems([directory]),
                selection: new Set<string>(),
                maxSize: 1,
                viewportHeight: 600,
                path: '/dir',
                onMove,
                onSelect: vi.fn(),
                onOpen: vi.fn(),
                onContextMenu: vi.fn(),
            },
        });
        const row = container.querySelector('[data-path="/dir/sub"]') as HTMLElement;
        beginDrag(['/other/a']);
        const dataTransfer = fakeDataTransfer();

        await fireEvent.dragOver(row, { dataTransfer });
        await fireEvent.drop(row, { dataTransfer });

        expect(onMove).toHaveBeenCalledTimes(1);
        expect(onMove).toHaveBeenCalledWith(['/other/a'], '/dir/sub');
    });
});

describe('FileList grouped deep-search headers', () => {
    function groupedItems(): ListItem[] {
        return [
            { kind: 'header', path: '/search' },
            { kind: 'entry', entry: { ...makeEntry(0), name: 'root.txt', path: '/search/root.txt' } },
            { kind: 'header', path: '/search/sub' },
            { kind: 'entry', entry: { ...makeEntry(1), name: 'nested.txt', path: '/search/sub/nested.txt' } },
        ];
    }

    function renderGrouped(onGroupNavigate = vi.fn(), onSelect = vi.fn()) {
        return {
            onGroupNavigate,
            onSelect,
            ...render(FileList, {
                props: {
                    items: groupedItems(),
                    selection: new Set<string>(),
                    maxSize: 10,
                    viewportHeight: 600,
                    onGroupNavigate,
                    onSelect,
                    onOpen: vi.fn(),
                    onContextMenu: vi.fn(),
                },
            }),
        };
    }

    it('renders one header row per group, each 30px tall, with a folder icon', () => {
        const { container } = renderGrouped();
        const headers = container.querySelectorAll('.group-header');
        expect(headers.length).toBe(2);
        for (const header of headers) {
            expect((header as HTMLElement).querySelector('.icon')).not.toBeNull();
        }
    });

    it('labels the root-level group "." and a nested group by its relative path', () => {
        const { container } = render(FileList, {
            props: {
                items: groupedItems(),
                selection: new Set<string>(),
                maxSize: 10,
                viewportHeight: 600,
                path: '/search',
                onSelect: vi.fn(),
                onOpen: vi.fn(),
                onContextMenu: vi.fn(),
            },
        });
        const labels = [...container.querySelectorAll('.group-label')].map((el) => el.textContent);
        expect(labels).toEqual(['.', 'sub']);
    });

    it('indents entry rows that belong to a group, without indenting headers', () => {
        const { container } = renderGrouped();
        const rows = container.querySelectorAll('.frow');
        expect(rows.length).toBe(2);
        for (const row of rows) {
            expect((row as HTMLElement).classList.contains('indented')).toBe(true);
        }
    });

    it('does not indent rows when the list is not grouped', () => {
        const { container } = renderList(makeEntries(2), 600);
        const row = container.querySelector('.frow') as HTMLElement;
        expect(row.classList.contains('indented')).toBe(false);
    });

    it('clicking a header calls onGroupNavigate with the absolute group path and never onSelect', async () => {
        const { container, onGroupNavigate, onSelect } = renderGrouped();
        const header = container.querySelectorAll('.group-header')[1] as HTMLElement;
        await fireEvent.click(header);
        expect(onGroupNavigate).toHaveBeenCalledTimes(1);
        expect(onGroupNavigate).toHaveBeenCalledWith('/search/sub');
        expect(onSelect).not.toHaveBeenCalled();
    });

    it('gives header rows no selection highlight even when their path matches the selection', () => {
        const { container } = render(FileList, {
            props: {
                items: groupedItems(),
                selection: new Set<string>(['/search']),
                maxSize: 10,
                viewportHeight: 600,
                onSelect: vi.fn(),
                onOpen: vi.fn(),
                onContextMenu: vi.fn(),
            },
        });
        const header = container.querySelectorAll('.group-header')[0] as HTMLElement;
        expect(header.classList.contains('sel')).toBe(false);
    });

    it('counts header rows toward the virtualization row height when computing the sizer', () => {
        const { container } = renderGrouped();
        const sizer = container.querySelector('.sizer') as HTMLElement;
        // 4 total items (2 headers + 2 entries), 30px each.
        expect(sizer.style.height).toBe('120px');
    });
});
