<script lang="ts">
    /**
     * The automatic preview pane. It appears when the single-selected entry is
     * previewable (image, document, or code) and shows a thumbnail — a rendered
     * image for image content, otherwise a content icon with a text snippet in a
     * `<pre>` — above the entry name and a metadata block. There is no manual
     * toggle: the App decides whether to allocate the 250px column via
     * `isPreviewable`, and this component guards the null and non-previewable
     * cases itself.
     *
     * Preview loads are async and the selection can change quickly, so a
     * request-sequence guard drops stale results: a plain (non-reactive) counter
     * is incremented each time `entry` changes; a resolved `preview` promise is
     * applied only if its captured sequence still matches the current one. The
     * displayed preview resets immediately on change so a stale thumbnail never
     * lingers, and a rejected `preview` leaves the metadata visible without a
     * thumbnail.
     */
    import type { FileEntry, PreviewPayload, PermissionClass, ContentKind } from '@quantum/client';
    import Icon, { type IconName } from './Icon.svelte';
    import { isPreviewable } from './preview';
    import { formatSize, formatModified } from './format';

    interface Props {
        /** The current single-selected entry, or null for none / multi-select. */
        entry: FileEntry | null;
        /** The typed IPC surface; only `preview` is used here. */
        ipc: { preview(path: string): Promise<PreviewPayload> };
    }

    const { entry, ipc }: Props = $props();

    // Content classification to the icon shown in the thumbnail when there is no
    // rendered image, mirroring the row icon mapping.
    const CONTENT_ICON: Record<ContentKind, IconName> = {
        image: 'image',
        code: 'code',
        music: 'music',
        archive: 'archive',
        document: 'document',
        other: 'file',
    };

    // Permission class to the label and colour shown in the metadata block,
    // mirroring the design playground. `normal` has neither.
    const PERMISSION_LABEL: Record<PermissionClass, string> = {
        executable: 'executable',
        read_only: 'read-only',
        root_owned: 'root-owned',
        normal: '',
    };
    const PERMISSION_COLOR: Record<PermissionClass, string> = {
        executable: 'var(--color-accent)',
        read_only: 'var(--color-warning)',
        root_owned: 'var(--color-error)',
        normal: '',
    };

    // The resolved preview payload for the current entry, or null while loading,
    // when none applies, or when the load failed.
    let preview = $state<PreviewPayload | null>(null);

    // A plain, non-reactive counter incremented on every entry change. It guards
    // against stale async resolves and must NOT be `$state`: reading a reactive
    // counter inside the effect that also writes it would loop.
    let requestSequence = 0;

    $effect(() => {
        const current = entry;
        requestSequence += 1;
        const mySequence = requestSequence;
        // Reset immediately so a previous entry's thumbnail does not linger while
        // the new one loads.
        preview = null;
        if (current === null || !isPreviewable(current)) {
            return;
        }
        ipc.preview(current.path)
            .then((payload) => {
                // Apply only if the selection has not moved on since this request.
                if (mySequence === requestSequence) {
                    preview = payload;
                }
            })
            .catch(() => {
                // A failed preview leaves the metadata visible without a thumbnail.
            });
    });

    const previewable = $derived(entry !== null && isPreviewable(entry));
    const iconName = $derived(entry === null ? 'file' : CONTENT_ICON[entry.content_kind]);
    const typeLabel = $derived(entry === null ? '' : entry.content_kind);
    const sizeLabel = $derived(entry === null ? '' : formatSize(entry.recursive_size ?? entry.size));
    const modifiedLabel = $derived(
        entry === null ? '' : formatModified(entry.modified_epoch_seconds),
    );
    const permissionLabel = $derived(entry === null ? '' : PERMISSION_LABEL[entry.permission_class]);
    const permissionColor = $derived(entry === null ? '' : PERMISSION_COLOR[entry.permission_class]);
    const showImage = $derived(
        entry !== null && entry.content_kind === 'image' && preview?.kind === 'image',
    );
    const showText = $derived(preview?.kind === 'text');
</script>

{#if entry === null}
    <div class="pane">
        <div class="empty">Select a file to preview</div>
    </div>
{:else if previewable}
    <div class="pane">
        <div class="pthumb">
            {#if showImage && preview !== null}
                <img src={preview.data} alt={entry.name} />
            {:else}
                <Icon name={iconName} size={48} />
            {/if}
        </div>
        <h3>{entry.name}</h3>
        <div class="meta">
            <div class="m"><span class="k">Type</span><span class="v">{typeLabel}</span></div>
            <div class="m"><span class="k">Size</span><span class="v">{sizeLabel}</span></div>
            <div class="m"><span class="k">Owner</span><span class="v">{entry.owner}</span></div>
            <div class="m">
                <span class="k">Permissions</span>
                <span class="v">
                    {entry.permissions}{#if permissionLabel}
                        <span style="color: {permissionColor}">({permissionLabel})</span>
                    {/if}
                </span>
            </div>
            <div class="m"><span class="k">Modified</span><span class="v">{modifiedLabel}</span></div>
        </div>
        {#if showText && preview !== null}
            <pre>{preview.data}</pre>
        {/if}
    </div>
{/if}

<style>
    .pane {
        flex: none;
        width: 250px;
        border-left: 1px solid var(--color-border);
        overflow-y: auto;
        padding: 14px;
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .pthumb {
        width: 100%;
        aspect-ratio: 4 / 3;
        border-radius: 10px;
        background: var(--color-bg-alt);
        display: flex;
        align-items: center;
        justify-content: center;
        overflow: hidden;
    }
    .pthumb img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }
    .pthumb :global(svg) {
        width: 48px;
        height: 48px;
        opacity: 0.6;
    }
    h3 {
        font-size: 13px;
        word-break: break-all;
        margin: 0;
    }
    .meta {
        display: flex;
        flex-direction: column;
        gap: 5px;
        font-size: 11px;
    }
    .meta .m {
        display: flex;
        justify-content: space-between;
        gap: 8px;
    }
    .meta .k {
        color: var(--color-muted);
    }
    .meta .v {
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 10px;
        text-align: right;
    }
    pre {
        background: var(--color-bg-alt);
        border: 1px solid var(--color-border);
        border-radius: 8px;
        padding: 8px;
        font-size: 10px;
        line-height: 1.5;
        font-family: var(--font-mono, ui-monospace, monospace);
        color: var(--color-fg-alt);
        overflow: hidden;
        max-height: 180px;
        margin: 0;
        white-space: pre-wrap;
    }
    .empty {
        color: var(--color-muted);
        font-size: 11px;
        text-align: center;
        margin-top: 30px;
    }
</style>
