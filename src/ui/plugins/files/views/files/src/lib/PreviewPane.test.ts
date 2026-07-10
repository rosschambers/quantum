import { describe, it, expect, vi } from 'vitest';
import { tick } from 'svelte';
import { render } from '@testing-library/svelte/svelte5';
import type { FileEntry, PreviewPayload } from '@quantum/client';
import PreviewPane from './PreviewPane.svelte';
import { isPreviewable } from './preview';
import { formatSize, formatModified } from './format';

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

/** A promise whose resolution is triggered manually, for ordering-controlled race tests. */
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((res) => {
        resolve = res;
    });
    return { promise, resolve };
}

/** Let queued microtasks and Svelte effects settle after an async state update. */
async function settle(): Promise<void> {
    await tick();
    await Promise.resolve();
    await tick();
}

describe('isPreviewable', () => {
    it('is true for image, document, and code files', () => {
        expect(isPreviewable(entry({ name: 'a', path: '/a', content_kind: 'image' }))).toBe(true);
        expect(isPreviewable(entry({ name: 'b', path: '/b', content_kind: 'document' }))).toBe(true);
        expect(isPreviewable(entry({ name: 'c', path: '/c', content_kind: 'code' }))).toBe(true);
    });

    it('is false for other, music, and archive files', () => {
        expect(isPreviewable(entry({ name: 'a', path: '/a', content_kind: 'other' }))).toBe(false);
        expect(isPreviewable(entry({ name: 'b', path: '/b', content_kind: 'music' }))).toBe(false);
        expect(isPreviewable(entry({ name: 'c', path: '/c', content_kind: 'archive' }))).toBe(false);
    });

    it('is false for a directory even when its content kind is previewable', () => {
        expect(
            isPreviewable(
                entry({ name: 'd', path: '/d', kind: 'directory', content_kind: 'image' }),
            ),
        ).toBe(false);
    });
});

/** A fake ipc whose `preview` resolves a fixed payload and records calls. */
function fakeIpc(payload: PreviewPayload) {
    const preview = vi.fn((_path: string): Promise<PreviewPayload> => Promise.resolve(payload));
    return { preview } as unknown as { preview: typeof preview };
}

describe('PreviewPane', () => {
    it('renders an image thumbnail for an image entry once preview resolves', async () => {
        const ipc = fakeIpc({ kind: 'image', data: 'data:image/png;base64,AAAA' });
        const imageEntry = entry({ name: 'pic.png', path: '/pic.png', content_kind: 'image' });
        const { container } = render(PreviewPane, { props: { entry: imageEntry, ipc } });

        await settle();

        const image = container.querySelector('img') as HTMLImageElement | null;
        expect(image).not.toBeNull();
        expect(image?.getAttribute('src')).toBe('data:image/png;base64,AAAA');
        expect(ipc.preview).toHaveBeenCalledWith('/pic.png');
    });

    it('renders a text snippet in a pre for a code entry', async () => {
        const ipc = fakeIpc({ kind: 'text', data: 'hello source' });
        const codeEntry = entry({ name: 'main.rs', path: '/main.rs', content_kind: 'code' });
        const { container } = render(PreviewPane, { props: { entry: codeEntry, ipc } });

        await settle();

        const pre = container.querySelector('pre');
        expect(pre).not.toBeNull();
        expect(pre?.textContent).toContain('hello source');
    });

    it('shows the empty hint and never calls preview when the entry is null', async () => {
        const ipc = fakeIpc({ kind: 'none', data: '' });
        const { container } = render(PreviewPane, { props: { entry: null, ipc } });

        await settle();

        expect(container.querySelector('.empty')).not.toBeNull();
        expect(container.textContent).toContain('Select a file to preview');
        expect(ipc.preview).not.toHaveBeenCalled();
    });

    it('drops a stale preview when the selection changes before it resolves', async () => {
        const alpha = deferred<PreviewPayload>();
        const beta = deferred<PreviewPayload>();
        const byPath: Record<string, Promise<PreviewPayload>> = {
            '/alpha.rs': alpha.promise,
            '/beta.rs': beta.promise,
        };
        const preview = vi.fn((path: string): Promise<PreviewPayload> => byPath[path]);
        const ipc = { preview } as unknown as { preview: typeof preview };

        const entryA = entry({ name: 'alpha.rs', path: '/alpha.rs', content_kind: 'code' });
        const entryB = entry({ name: 'beta.rs', path: '/beta.rs', content_kind: 'code' });

        const { container, rerender } = render(PreviewPane, { props: { entry: entryA, ipc } });
        await settle();

        // Selection moves to B before A's slow preview resolves.
        await rerender({ entry: entryB, ipc });
        await settle();

        // B resolves first, then the stale A resolves afterwards.
        beta.resolve({ kind: 'text', data: 'BETA CONTENT' });
        await settle();
        alpha.resolve({ kind: 'text', data: 'ALPHA CONTENT' });
        await settle();

        const pre = container.querySelector('pre');
        expect(pre?.textContent).toContain('BETA CONTENT');
        expect(pre?.textContent).not.toContain('ALPHA CONTENT');
    });

    it('renders metadata for a previewable entry', async () => {
        const ipc = fakeIpc({ kind: 'text', data: 'body' });
        const codeEntry = entry({
            name: 'report.md',
            path: '/report.md',
            content_kind: 'document',
            size: 2048,
            owner: 'ross',
            modified_epoch_seconds: 1_700_000_000,
        });
        const { container } = render(PreviewPane, { props: { entry: codeEntry, ipc } });

        await settle();

        const text = container.textContent ?? '';
        expect(text).toContain(formatSize(2048));
        expect(text).toContain('ross');
        expect(text).toContain(formatModified(1_700_000_000));
    });

    it('renders metadata without a thumbnail when preview rejects', async () => {
        const preview = vi.fn((_path: string): Promise<PreviewPayload> => Promise.reject(new Error('boom')));
        const ipc = { preview } as unknown as { preview: typeof preview };
        const imageEntry = entry({
            name: 'pic.png',
            path: '/pic.png',
            content_kind: 'image',
            owner: 'ross',
        });
        const { container } = render(PreviewPane, { props: { entry: imageEntry, ipc } });

        await settle();

        expect(container.querySelector('img')).toBeNull();
        expect(container.textContent ?? '').toContain('ross');
    });
});
