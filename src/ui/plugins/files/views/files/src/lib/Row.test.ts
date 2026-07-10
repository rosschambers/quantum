import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import type { FileEntry } from '@quantum/client';
import Row from './Row.svelte';
import { formatSize, formatModified } from './format';

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
