<script lang="ts">
    /**
     * A single fixed-height (30px) file row: a content icon, the entry name
     * tinted by permission class (executable/read-only/root-owned) and set
     * italic with a target suffix for symlinks, a size column with a mini
     * usage bar, and a modified-time column. The parent (FileList / App) owns
     * all selection and navigation logic; this component only renders and
     * reports pointer events, passing the raw MouseEvent up so the parent can
     * read modifier keys. Rows are draggable and directory rows are drop
     * targets; the drag payload travels through the shared `dragState` module
     * rather than `dataTransfer` (Task 23).
     */
    import type { FileEntry, ContentKind, PermissionClass } from '@quantum/client';
    import Icon, { type IconName } from './Icon.svelte';
    import { formatSize, formatModified } from './format';
    import { beginDrag, endDrag, getDragSources } from './dragState.svelte';
    import { isValidDrop } from './dnd';

    interface Props {
        entry: FileEntry;
        selected: boolean;
        /** The largest sibling size, used to scale the mini usage bar. */
        maxSize: number;
        onSelect: (event: MouseEvent) => void;
        onOpen: () => void;
        onContextMenu: (event: MouseEvent) => void;
        /**
         * The selection-aware source paths to drag when this row is already
         * part of the selection. Defaults to just this row's own path.
         */
        dragSources?: () => string[];
        /** Move dropped sources into this row's directory (directory rows only). */
        onMove?: (sources: string[], destination: string) => void;
        /**
         * True while this row's recursive size is still being calculated,
         * rendering a small dot before the size text. FileList only passes
         * true for directory rows; this component does not re-check the kind
         * and trusts the caller, so a file with `calculating` true would still
         * render a dot — the caller simply never does that.
         */
        calculating?: boolean;
    }

    const {
        entry,
        selected,
        maxSize,
        onSelect,
        onOpen,
        onContextMenu,
        dragSources,
        onMove,
        calculating = false,
    }: Props = $props();

    // Content-kind to icon glyph. Directories and symlinks take precedence over
    // the content classification.
    const CONTENT_ICON: Record<ContentKind, IconName> = {
        image: 'image',
        code: 'code',
        music: 'music',
        archive: 'archive',
        document: 'document',
        other: 'file',
    };

    const iconName = $derived.by((): IconName => {
        if (entry.kind === 'directory') {
            return 'folder';
        }
        if (entry.kind === 'symlink') {
            return 'link';
        }
        return CONTENT_ICON[entry.content_kind];
    });

    // Permission class to the tint class the playground uses on the name.
    const PERMISSION_CLASS: Record<PermissionClass, string> = {
        executable: 'p-exec',
        read_only: 'p-ro',
        root_owned: 'p-root',
        normal: '',
    };

    const permissionClass = $derived(PERMISSION_CLASS[entry.permission_class]);
    const isSymlink = $derived(entry.kind === 'symlink');

    // The size shown in the size column: recursive size when known, else the
    // plain size.
    const displaySize = $derived(formatSize(entry.recursive_size ?? entry.size));

    // The value that drives the mini bar. A directory with an unknown
    // (null) recursive size contributes nothing, so its bar is empty rather
    // than borrowing its own on-disk inode size.
    const sizeForBar = $derived(
        entry.recursive_size !== null
            ? entry.recursive_size
            : entry.kind === 'directory'
              ? 0
              : entry.size,
    );

    const barWidth = $derived(maxSize > 0 ? (sizeForBar / maxSize) * 100 : 0);

    const modified = $derived(formatModified(entry.modified_epoch_seconds));

    const isDirectory = $derived(entry.kind === 'directory');

    // True while a valid drag hovers this directory row, driving the outline.
    let isDropTarget = $state(false);

    function handleContextMenu(event: MouseEvent): void {
        event.preventDefault();
        onContextMenu(event);
    }

    // Begin a drag. An unselected row first becomes the sole selection (a plain
    // drag event carries no modifier keys, so the parent selects only it), and
    // the payload is just this row. An already-selected row drags the whole
    // selection-aware set. The paths also go on `dataTransfer` for completeness.
    function handleDragStart(event: DragEvent): void {
        let sources: string[];
        if (selected) {
            sources = dragSources ? dragSources() : [entry.path];
        } else {
            onSelect(event);
            sources = [entry.path];
        }
        beginDrag(sources);
        event.dataTransfer?.setData('text/plain', JSON.stringify(sources));
    }

    function handleDragEnd(): void {
        endDrag();
    }

    // A directory row accepts a drop only for a valid move (destination not a
    // source and not inside a source). `stopPropagation` keeps the pane's list
    // background from also handling the same drop.
    function handleDragOver(event: DragEvent): void {
        if (!isDirectory) {
            return;
        }
        const sources = getDragSources();
        if (sources === null || !isValidDrop(sources, entry.path)) {
            return;
        }
        event.preventDefault();
        event.stopPropagation();
        isDropTarget = true;
    }

    function handleDragLeave(): void {
        isDropTarget = false;
    }

    function handleDrop(event: DragEvent): void {
        if (!isDirectory) {
            return;
        }
        const sources = getDragSources();
        if (sources === null || !isValidDrop(sources, entry.path)) {
            return;
        }
        event.preventDefault();
        event.stopPropagation();
        isDropTarget = false;
        onMove?.(sources, entry.path);
        endDrag();
    }
</script>

<!--
    Keyboard is handled at the App level (Task 24), so this row intentionally
    has no key handler; the a11y hint is suppressed rather than adding dead
    key logic here.
-->
<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
<div
    class="frow"
    class:sel={selected}
    class:droptarget={isDropTarget}
    data-path={entry.path}
    draggable={true}
    onclick={onSelect}
    ondblclick={onOpen}
    oncontextmenu={handleContextMenu}
    ondragstart={handleDragStart}
    ondragend={handleDragEnd}
    ondragover={handleDragOver}
    ondragleave={handleDragLeave}
    ondrop={handleDrop}
    role="row"
    tabindex="-1"
>
    <span class="fico"><Icon name={iconName} size={15} /></span>
    <span class="nm {permissionClass}" class:p-link={isSymlink}>
        <span class="label">{entry.name}</span>
        {#if isSymlink && entry.symlink_target}
            <span class="sub">&rarr; {entry.symlink_target}</span>
        {/if}
    </span>
    <span class="sz">
        <span class="szval">
            {#if calculating}<span
                    class="size-calculating"
                    title="Calculating"
                    aria-label="Calculating"
                ></span>{/if}{displaySize}
        </span>
        <span class="minibar"><i style="width: {barWidth}%"></i></span>
    </span>
    <span class="mt">{modified}</span>
</div>

<style>
    .frow {
        display: flex;
        align-items: center;
        gap: 8px;
        height: 30px;
        padding: 0 6px;
        border-radius: 6px;
        font-size: 12px;
        color: var(--color-fg-alt);
        cursor: default;
        user-select: none;
        position: relative;
        overflow: hidden;
    }
    .frow:hover {
        background: var(--color-surface-hover, hsla(230, 14%, 42%, 1));
    }
    .frow.sel {
        background: color-mix(in oklab, var(--color-accent) 16%, transparent);
        color: var(--color-fg);
    }
    .frow.droptarget {
        outline: 1px dashed var(--color-accent);
        outline-offset: -1px;
    }
    .fico {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        opacity: 0.85;
    }
    .nm {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .nm .label {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .nm .sub {
        font-size: 10px;
        color: var(--color-muted);
        font-family: var(--font-mono, ui-monospace, monospace);
        flex: none;
    }
    .sz {
        width: 130px;
        flex: none;
        text-align: right;
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 11px;
        color: var(--color-muted);
        display: flex;
        flex-direction: column;
        align-items: flex-end;
        justify-content: center;
        gap: 2px;
    }
    .size-calculating {
        display: inline-block;
        width: 8px;
        height: 8px;
        border-radius: 50%;
        background: var(--color-accent);
        margin-right: 6px;
        vertical-align: middle;
    }
    .sz .minibar {
        width: 64px;
        height: 3px;
        border-radius: 2px;
        background: var(--color-bg-alt);
        overflow: hidden;
    }
    .sz .minibar i {
        display: block;
        height: 100%;
        background: var(--color-accent);
        opacity: 0.7;
    }
    .mt {
        width: 130px;
        flex: none;
        text-align: right;
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 10px;
        color: var(--color-muted);
    }
    /* Permission tints, mirroring the design playground. */
    .p-exec {
        color: var(--color-accent);
    }
    .p-ro {
        color: var(--color-warning);
    }
    .p-root {
        color: var(--color-error);
    }
    .p-link {
        font-style: italic;
    }
</style>
