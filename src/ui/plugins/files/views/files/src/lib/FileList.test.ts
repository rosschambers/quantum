import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import type { FileEntry } from '@quantum/client';
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

function renderList(entries: FileEntry[], viewportHeight: number) {
    return render(FileList, {
        props: {
            entries,
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
                entries: [] as FileEntry[],
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
});
