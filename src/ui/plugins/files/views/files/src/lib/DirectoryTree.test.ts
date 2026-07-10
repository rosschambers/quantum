import { describe, it, expect, vi } from 'vitest';
import { tick } from 'svelte';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import type { FileEntry } from '@quantum/client';
import DirectoryTree from './DirectoryTree.svelte';
import { ancestorPaths } from './path';

/** Build a `FileEntry` with sensible defaults, overriding only what a test cares about. */
function entry(overrides: Partial<FileEntry> & { name: string; path: string }): FileEntry {
    return {
        name: overrides.name,
        path: overrides.path,
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

/** A fake `list` that returns a fixed set per path and records how often each path was listed. */
function fakeIpc(byPath: Record<string, FileEntry[]>) {
    const list = vi.fn((path: string): Promise<FileEntry[]> => {
        return Promise.resolve(byPath[path] ?? []);
    });
    return { list } as unknown as { list: typeof list };
}

/** Let queued microtasks and Svelte effects settle after an async state update. */
async function settle(): Promise<void> {
    await tick();
    await Promise.resolve();
    await tick();
}

const TREE: Record<string, FileEntry[]> = {
    '/': [
        entry({ name: 'home', path: '/home', kind: 'directory' }),
        entry({ name: 'etc', path: '/etc', kind: 'directory' }),
        entry({ name: 'readme.txt', path: '/readme.txt', kind: 'file' }),
    ],
    '/home': [
        entry({ name: 'user', path: '/home/user', kind: 'directory' }),
        entry({ name: '.config', path: '/home/.config', kind: 'directory' }),
        entry({ name: 'notes.md', path: '/home/notes.md', kind: 'file' }),
    ],
};

function rowByPath(container: HTMLElement, path: string): HTMLElement | null {
    return container.querySelector(`.tree-row[data-path="${path}"]`);
}

describe('ancestorPaths', () => {
    it('returns just the root for the root path', () => {
        expect(ancestorPaths('/')).toEqual(['/']);
    });

    it('accumulates each ancestor including the leaf', () => {
        expect(ancestorPaths('/home/user/x')).toEqual(['/', '/home', '/home/user', '/home/user/x']);
    });

    it('ignores a trailing slash', () => {
        expect(ancestorPaths('/home/user/')).toEqual(['/', '/home', '/home/user']);
    });
});

describe('DirectoryTree lazy loading', () => {
    it('lists a node once on expand and shows only directory children', async () => {
        const ipc = fakeIpc(TREE);
        const { container } = render(DirectoryTree, {
            props: { ipc, activePath: '/', onNavigate: vi.fn() },
        });
        await settle();

        // The root auto-expanded (it is the active path's only ancestor): its
        // directory children render, its file child does not.
        expect(rowByPath(container, '/home')).not.toBeNull();
        expect(rowByPath(container, '/etc')).not.toBeNull();
        expect(rowByPath(container, '/readme.txt')).toBeNull();
        expect(ipc.list).toHaveBeenCalledWith('/');

        // Expand /home via its chevron: it lists once and shows its directory
        // children, including the dotfile directory, but not its file child.
        const chevron = rowByPath(container, '/home')?.querySelector('.chev') as HTMLElement;
        await fireEvent.click(chevron);
        await settle();

        expect(ipc.list).toHaveBeenCalledWith('/home');
        expect(rowByPath(container, '/home/user')).not.toBeNull();
        expect(rowByPath(container, '/home/.config')).not.toBeNull();
        expect(rowByPath(container, '/home/notes.md')).toBeNull();

        const listCallsForHome = ipc.list.mock.calls.filter((call) => call[0] === '/home').length;
        expect(listCallsForHome).toBe(1);

        // Collapse then re-expand: the cache is used, so no second fetch.
        await fireEvent.click(chevron);
        await settle();
        await fireEvent.click(chevron);
        await settle();

        expect(rowByPath(container, '/home/user')).not.toBeNull();
        const listCallsForHomeAfter = ipc.list.mock.calls.filter((call) => call[0] === '/home').length;
        expect(listCallsForHomeAfter).toBe(1);
    });

    it('highlights the active-path row and auto-expands its ancestors', async () => {
        const ipc = fakeIpc(TREE);
        const { container } = render(DirectoryTree, {
            props: { ipc, activePath: '/home/user', onNavigate: vi.fn() },
        });
        await settle();

        // The ancestor chain was expanded so the active node is visible.
        const activeRow = rowByPath(container, '/home/user');
        expect(activeRow).not.toBeNull();
        expect(activeRow?.classList.contains('active')).toBe(true);
        expect(rowByPath(container, '/home')?.classList.contains('active')).toBe(false);
    });

    it('navigates when a row body (not the chevron) is clicked', async () => {
        const ipc = fakeIpc(TREE);
        const onNavigate = vi.fn();
        const { container } = render(DirectoryTree, {
            props: { ipc, activePath: '/', onNavigate },
        });
        await settle();

        const row = rowByPath(container, '/home') as HTMLElement;
        await fireEvent.click(row);
        expect(onNavigate).toHaveBeenCalledWith('/home');
    });
});
