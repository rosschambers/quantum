import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import type { FileEntry } from '@quantum/client';
import Row from './Row.svelte';
import { formatSize, formatModified } from './format';
import { beginDrag, endDrag, getDragSources } from './dragState.svelte';

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

function renderRow(e: FileEntry, extra: Partial<Record<string, unknown>> = {}) {
    return render(Row, {
        props: {
            entry: e,
            selected: false,
            maxSize: 1000,
            onSelect: vi.fn(),
            onOpen: vi.fn(),
            onContextMenu: vi.fn(),
            ...extra,
        },
    });
}

describe('formatSize', () => {
    it('formats zero and sub-kilobyte values in bytes', () => {
        expect(formatSize(0)).toBe('0 B');
        expect(formatSize(1023)).toBe('1023 B');
    });

    it('formats kilobyte thresholds without trailing zero', () => {
        expect(formatSize(1024)).toBe('1 KB');
        expect(formatSize(1536)).toBe('1.5 KB');
    });

    it('formats megabyte and gigabyte thresholds', () => {
        expect(formatSize(1024 * 1024)).toBe('1 MB');
        expect(formatSize(1.5 * 1024 * 1024)).toBe('1.5 MB');
        expect(formatSize(1024 * 1024 * 1024)).toBe('1 GB');
        expect(formatSize(2.4 * 1024 * 1024 * 1024)).toBe('2.4 GB');
    });
});

describe('formatModified', () => {
    it('formats a known epoch second as local YYYY-MM-DD HH:MM', () => {
        const seconds = 1_700_000_000;
        const date = new Date(seconds * 1000);
        const pad = (value: number): string => String(value).padStart(2, '0');
        const expected =
            `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
            `${pad(date.getHours())}:${pad(date.getMinutes())}`;
        expect(formatModified(seconds)).toBe(expected);
    });
});

describe('Row rendering', () => {
    it('renders a directory with the folder icon and no permission tint', () => {
        const { container } = renderRow(entry({ name: 'Documents', kind: 'directory' }));
        expect(container.querySelector('.frow')).not.toBeNull();
        const name = container.querySelector('.nm');
        expect(name?.classList.contains('p-exec')).toBe(false);
        expect(name?.classList.contains('p-ro')).toBe(false);
        expect(name?.classList.contains('p-root')).toBe(false);
    });

    it('renders a normal file with the default tint', () => {
        const { container, getByText } = renderRow(entry({ name: 'notes.txt' }));
        expect(getByText('notes.txt')).not.toBeNull();
        const name = container.querySelector('.nm');
        expect(name?.classList.contains('p-exec')).toBe(false);
    });

    it('tints an executable file with the executable class', () => {
        const { container } = renderRow(
            entry({ name: 'deploy.sh', permission_class: 'executable', content_kind: 'code' }),
        );
        expect(container.querySelector('.nm')?.classList.contains('p-exec')).toBe(true);
    });

    it('tints a read-only file with the read-only class', () => {
        const { container } = renderRow(
            entry({ name: 'taxes.pdf', permission_class: 'read_only', content_kind: 'document' }),
        );
        expect(container.querySelector('.nm')?.classList.contains('p-ro')).toBe(true);
    });

    it('tints a root-owned file with the root class', () => {
        const { container } = renderRow(
            entry({ name: 'shadow', permission_class: 'root_owned', owner: 'root' }),
        );
        expect(container.querySelector('.nm')?.classList.contains('p-root')).toBe(true);
    });

    it('renders a symlink italic with a target suffix', () => {
        const { container, getByText } = renderRow(
            entry({ name: 'latest', kind: 'symlink', symlink_target: '/home/user/target' }),
        );
        expect(container.querySelector('.nm')?.classList.contains('p-link')).toBe(true);
        expect(getByText(/→ \/home\/user\/target/)).not.toBeNull();
    });

    it('sets the mini bar width from the sizeForBar/maxSize ratio', () => {
        const { container } = renderRow(entry({ name: 'big', size: 250 }), { maxSize: 1000 });
        const bar = container.querySelector('.minibar i') as HTMLElement | null;
        expect(bar).not.toBeNull();
        expect(bar?.style.width).toBe('25%');
    });

    it('treats a directory with null recursive size as a zero-width bar', () => {
        const { container } = renderRow(
            entry({ name: 'empty', kind: 'directory', recursive_size: null }),
            { maxSize: 1000 },
        );
        const bar = container.querySelector('.minibar i') as HTMLElement | null;
        expect(bar?.style.width).toBe('0%');
    });

    it('invokes onSelect on click, onOpen on double-click, onContextMenu on right-click', async () => {
        const onSelect = vi.fn();
        const onOpen = vi.fn();
        const onContextMenu = vi.fn();
        const { container } = renderRow(entry({ name: 'thing' }), {
            onSelect,
            onOpen,
            onContextMenu,
        });
        const row = container.querySelector('.frow') as HTMLElement;
        await fireEvent.click(row);
        expect(onSelect).toHaveBeenCalledTimes(1);
        await fireEvent.dblClick(row);
        expect(onOpen).toHaveBeenCalledTimes(1);
        await fireEvent.contextMenu(row);
        expect(onContextMenu).toHaveBeenCalledTimes(1);
    });
});

describe('Row calculating indicator', () => {
    it('renders a calculating dot before the size for a directory when calculating', () => {
        const { container } = renderRow(
            entry({ name: 'Documents', kind: 'directory' }),
            { calculating: true },
        );
        const dot = container.querySelector('.size-calculating') as HTMLElement;
        expect(dot).not.toBeNull();
        expect(dot.getAttribute('title')).toBe('Calculating');

        // The dot must precede the size text within the size-value cell: the
        // dot node comes before the '0 B' text node in DOM order.
        const value = container.querySelector('.szval') as HTMLElement;
        expect(value).not.toBeNull();
        expect(value.textContent).toContain('0 B');
        const sizeText = [...value.childNodes].find(
            (node) => node.nodeType === Node.TEXT_NODE && node.textContent?.trim() !== '',
        );
        expect(sizeText).not.toBeUndefined();
        expect(
            dot.compareDocumentPosition(sizeText!) & Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
    });

    it('renders no calculating dot when calculating is false', () => {
        const { container } = renderRow(
            entry({ name: 'Documents', kind: 'directory' }),
            { calculating: false },
        );
        expect(container.querySelector('.size-calculating')).toBeNull();
    });

    it('renders no calculating dot when the calculating prop is omitted', () => {
        const { container } = renderRow(entry({ name: 'Documents', kind: 'directory' }));
        expect(container.querySelector('.size-calculating')).toBeNull();
    });
});

describe('Row drag source', () => {
    afterEach(() => endDrag());

    it('marks every row draggable', () => {
        const { container } = renderRow(entry({ name: 'thing' }));
        const row = container.querySelector('.frow') as HTMLElement;
        expect(row.getAttribute('draggable')).toBe('true');
    });

    it('selects only the row first when dragging an unselected row', async () => {
        const onSelect = vi.fn();
        const { container } = renderRow(entry({ name: 'thing', path: '/dir/thing' }), {
            selected: false,
            onSelect,
            // A selection-aware getter that must be ignored while unselected.
            dragSources: () => ['/dir/other', '/dir/thing'],
        });
        const row = container.querySelector('.frow') as HTMLElement;
        const dataTransfer = fakeDataTransfer();
        await fireEvent.dragStart(row, { dataTransfer });

        expect(onSelect).toHaveBeenCalledTimes(1);
        expect(getDragSources()).toEqual(['/dir/thing']);
        expect(dataTransfer.setData).toHaveBeenCalledWith(
            'text/plain',
            JSON.stringify(['/dir/thing']),
        );
    });

    it('drags the whole selection when the row is already selected', async () => {
        const onSelect = vi.fn();
        const { container } = renderRow(entry({ name: 'thing', path: '/dir/thing' }), {
            selected: true,
            onSelect,
            dragSources: () => ['/dir/thing', '/dir/other'],
        });
        const row = container.querySelector('.frow') as HTMLElement;
        const dataTransfer = fakeDataTransfer();
        await fireEvent.dragStart(row, { dataTransfer });

        // Already selected: no re-select, and the full selection is the payload.
        expect(onSelect).not.toHaveBeenCalled();
        expect(getDragSources()).toEqual(['/dir/thing', '/dir/other']);
    });

    it('clears the drag payload on dragend', async () => {
        const { container } = renderRow(entry({ name: 'thing', path: '/dir/thing' }));
        const row = container.querySelector('.frow') as HTMLElement;
        await fireEvent.dragStart(row, { dataTransfer: fakeDataTransfer() });
        expect(getDragSources()).not.toBeNull();
        await fireEvent.dragEnd(row);
        expect(getDragSources()).toBeNull();
    });
});

describe('Row directory drop target', () => {
    afterEach(() => endDrag());

    it('calls onMove once with the drag sources and the directory path on a valid drop', async () => {
        const onMove = vi.fn();
        const { container } = renderRow(
            entry({ name: 'dest', path: '/dir/dest', kind: 'directory' }),
            { onMove },
        );
        const row = container.querySelector('.frow') as HTMLElement;
        beginDrag(['/other/a', '/other/b']);
        const dataTransfer = fakeDataTransfer();

        await fireEvent.dragOver(row, { dataTransfer });
        expect(row.classList.contains('droptarget')).toBe(true);

        await fireEvent.drop(row, { dataTransfer });
        expect(onMove).toHaveBeenCalledTimes(1);
        expect(onMove).toHaveBeenCalledWith(['/other/a', '/other/b'], '/dir/dest');
        expect(row.classList.contains('droptarget')).toBe(false);
        expect(getDragSources()).toBeNull();
    });

    it('does nothing when the destination is one of the sources', async () => {
        const onMove = vi.fn();
        const { container } = renderRow(
            entry({ name: 'a', path: '/dir/a', kind: 'directory' }),
            { onMove },
        );
        const row = container.querySelector('.frow') as HTMLElement;
        beginDrag(['/dir/a', '/dir/b']);
        const dataTransfer = fakeDataTransfer();

        await fireEvent.dragOver(row, { dataTransfer });
        expect(row.classList.contains('droptarget')).toBe(false);

        await fireEvent.drop(row, { dataTransfer });
        expect(onMove).not.toHaveBeenCalled();
    });

    it('removes the droptarget outline on dragleave', async () => {
        const { container } = renderRow(
            entry({ name: 'dest', path: '/dir/dest', kind: 'directory' }),
            { onMove: vi.fn() },
        );
        const row = container.querySelector('.frow') as HTMLElement;
        beginDrag(['/other/a']);
        const dataTransfer = fakeDataTransfer();

        await fireEvent.dragOver(row, { dataTransfer });
        expect(row.classList.contains('droptarget')).toBe(true);
        await fireEvent.dragLeave(row);
        expect(row.classList.contains('droptarget')).toBe(false);
    });

    it('is not a drop target when the row is a file', async () => {
        const onMove = vi.fn();
        const { container } = renderRow(entry({ name: 'file', path: '/dir/file' }), { onMove });
        const row = container.querySelector('.frow') as HTMLElement;
        beginDrag(['/other/a']);

        await fireEvent.dragOver(row, { dataTransfer: fakeDataTransfer() });
        expect(row.classList.contains('droptarget')).toBe(false);
        await fireEvent.drop(row, { dataTransfer: fakeDataTransfer() });
        expect(onMove).not.toHaveBeenCalled();
    });
});
