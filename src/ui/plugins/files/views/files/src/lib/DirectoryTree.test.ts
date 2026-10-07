import '../testSvelteRuntime';
import { describe, it, expect, vi, afterEach } from 'vitest';
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

describe('DirectoryTree viewport', () => {
    afterEach(() => vi.restoreAllMocks());

    function mountLargeTree(activePath = '/') {
        const sidebar = document.createElement('div');
        sidebar.className = 'sidebar';
        document.body.append(sidebar);
        Object.defineProperty(sidebar, 'clientHeight', { value: 200 });
        vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function () {
            const top = this.classList.contains('tree') ? 40 - sidebar.scrollTop : 0;
            return { top, bottom: top + 200, left: 0, right: 200, width: 200, height: 200, x: 0, y: top, toJSON() {} };
        });
        const directories = Array.from({ length: 3000 }, (_, index) =>
            entry({ name: `directory-${index}`, path: `/directory-${index}`, kind: 'directory' }),
        );
        const ipc = fakeIpc({
            '/': directories,
            '/directory-1500': [entry({ name: 'child', path: '/directory-1500/child', kind: 'directory' })],
        });
        const onNavigate = vi.fn();
        const rendered = render(DirectoryTree, { target: sidebar, props: { ipc, activePath, onNavigate } });
        return { ...rendered, sidebar, ipc, onNavigate };
    }

    it('mounts a bounded DOM subset and expands the correct offscreen node after scrolling', async () => {
        const { container, sidebar, ipc, onNavigate } = mountLargeTree();
        await settle();
        expect(container.querySelectorAll('.tree-row').length).toBeLessThanOrEqual(30);
        expect(container.querySelector('.tree')?.getAttribute('style')).toContain('60020px');
        expect(ipc.list).toHaveBeenCalledTimes(1);
        sidebar.scrollTop = 40 + 1501 * 20;
        await fireEvent.scroll(sidebar);
        const row = rowByPath(container, '/directory-1500')!;
        expect(row).not.toBeNull();
        await fireEvent.click(row.querySelector('.chev')!);
        await settle();
        expect(ipc.list).toHaveBeenCalledWith('/directory-1500');
        expect(rowByPath(container, '/directory-1500/child')).not.toBeNull();
        expect(onNavigate).not.toHaveBeenCalled();
        await fireEvent.click(row);
        expect(onNavigate).toHaveBeenCalledWith('/directory-1500');
        expect(container.querySelectorAll('.tree-row').length).toBeLessThanOrEqual(30);
        sidebar.remove();
    });

    it('reveals the selected current path without mounting its thousands of preceding siblings', async () => {
        const { container, sidebar, ipc } = mountLargeTree('/directory-1500/child');
        await settle();
        expect(sidebar.scrollTop).toBeGreaterThan(29000);
        expect(rowByPath(container, '/directory-1500/child')?.classList.contains('active')).toBe(true);
        expect(container.querySelectorAll('.tree-row').length).toBeLessThanOrEqual(30);
        expect(ipc.list.mock.calls.map(([path]) => path)).toEqual(['/', '/directory-1500', '/directory-1500/child']);
        sidebar.remove();
    });

    it('uses the same row height for rendered rows and the complete scroll range', async () => {
        const { container, sidebar } = mountLargeTree();
        await settle();
        const tree = container.querySelector<HTMLElement>('.tree')!;
        const row = container.querySelector<HTMLElement>('.tree-row')!;
        expect(Number.parseFloat(row.style.height)).toBe(Number.parseFloat(tree.style.height) / 3001);
        sidebar.scrollTop = 40 + 3001 * 20 - sidebar.clientHeight;
        await fireEvent.scroll(sidebar);
        expect(rowByPath(container, '/directory-2999')).not.toBeNull();
        expect(container.querySelectorAll('.tree-row').length).toBeLessThanOrEqual(30);
        sidebar.remove();
    });

    it('reveals a changed selection, then preserves manual scrolling and cached nested expansion', async () => {
        const { container, sidebar, ipc, rerender } = mountLargeTree();
        await settle();
        await rerender({ activePath: '/directory-1500/child' });
        await settle();
        expect(rowByPath(container, '/directory-1500/child')?.getAttribute('aria-level')).toBe('3');
        const chevron = rowByPath(container, '/directory-1500')!.querySelector('.chev')!;
        await fireEvent.click(chevron);
        expect(rowByPath(container, '/directory-1500/child')).toBeNull();
        await fireEvent.click(chevron);
        await settle();
        expect(rowByPath(container, '/directory-1500/child')).not.toBeNull();
        expect(ipc.list.mock.calls.filter(([path]) => path === '/directory-1500')).toHaveLength(1);
        sidebar.scrollTop = 40;
        await fireEvent.scroll(sidebar);
        expect(rowByPath(container, '/directory-1500/child')).toBeNull();
        expect(rowByPath(container, '/directory-0')).not.toBeNull();
        sidebar.remove();
    });
});
