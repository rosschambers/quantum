<script lang="ts">
    /**
     * The file/folder properties modal. It follows the shared overlay house
     * style (card on a blurred backdrop, dismiss on Escape or backdrop click,
     * no explicit close button) and shows an entry's name, path, type, size,
     * owner, permissions, and modified time. The App owns the open/closed state
     * and Escape handling at the window level; this component reports a backdrop
     * click through `onClose` and renders whatever fields the target carries.
     * A background-menu target only has a name and path, so every richer field
     * is optional and rendered only when present.
     */
    import type { FileEntry } from '@quantum/client';
    import { formatSize, formatModified } from './format';

    /** A properties target: always a name and path, optionally full entry metadata. */
    type PropertiesTarget = Partial<FileEntry> & { name: string; path: string };

    interface Props {
        target: PropertiesTarget;
        onClose: () => void;
    }

    const { target, onClose }: Props = $props();

    const sizeLabel = $derived(
        target.recursive_size != null
            ? formatSize(target.recursive_size)
            : target.size != null
              ? formatSize(target.size)
              : '',
    );
    const modifiedLabel = $derived(
        target.modified_epoch_seconds != null ? formatModified(target.modified_epoch_seconds) : '',
    );

    function handleBackdropClick(event: MouseEvent): void {
        if (event.target === event.currentTarget) {
            onClose();
        }
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={handleBackdropClick}>
    <div class="card properties-modal" role="dialog" aria-label="Properties">
        <h3>{target.name}</h3>
        <div class="meta">
            <div class="m"><span class="k">Path</span><span class="v">{target.path}</span></div>
            {#if target.kind}
                <div class="m"><span class="k">Type</span><span class="v">{target.kind}</span></div>
            {/if}
            {#if sizeLabel}
                <div class="m"><span class="k">Size</span><span class="v">{sizeLabel}</span></div>
            {/if}
            {#if target.owner}
                <div class="m"><span class="k">Owner</span><span class="v">{target.owner}</span></div>
            {/if}
            {#if target.permissions}
                <div class="m">
                    <span class="k">Permissions</span><span class="v">{target.permissions}</span>
                </div>
            {/if}
            {#if modifiedLabel}
                <div class="m">
                    <span class="k">Modified</span><span class="v">{modifiedLabel}</span>
                </div>
            {/if}
        </div>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        z-index: 300;
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--color-overlay-backdrop, rgba(0, 0, 0, 0.5));
        backdrop-filter: blur(4px);
    }
    .card {
        width: min(420px, 92vw);
        max-height: 80vh;
        overflow: hidden;
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding: 18px;
        background: var(--color-bg-alt);
        border: 1px solid var(--color-border);
        border-radius: 12px;
        box-shadow: 0 14px 40px var(--color-shadow);
    }
    h3 {
        font-size: 14px;
        word-break: break-all;
        margin: 0;
        color: var(--color-fg);
    }
    .meta {
        display: flex;
        flex-direction: column;
        gap: 6px;
        font-size: 12px;
        overflow-y: auto;
    }
    .m {
        display: flex;
        justify-content: space-between;
        gap: 12px;
    }
    .k {
        color: var(--color-muted);
        flex: none;
    }
    .v {
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 11px;
        text-align: right;
        word-break: break-all;
        color: var(--color-fg-alt);
    }
</style>
