<script lang="ts">
    /**
     * The 210px places sidebar: pinned locations, mounted drives with a fill
     * bar and free-space text, and a lazily-loaded directory tree. The parent
     * owns navigation and pin mutation; this component only renders and reports
     * intent through its callbacks. Right-clicking a pin opens a context menu
     * (Open / Open in other pane / Unpin) via the shared `openContextMenu`.
     */
    import { openContextMenu, type DriveInfo, type Pin } from '@quantum/client';
    import type { FileEntry } from '@quantum/client';
    import Icon from './Icon.svelte';
    import DirectoryTree from './DirectoryTree.svelte';
    import { formatSize } from './format';
    import { driveUsedFraction, driveBarClass } from './drive';
    import { getDragSources, endDrag } from './dragState.svelte';
    import { isValidDrop } from './dnd';

    interface TreeIpc {
        list(path: string): Promise<FileEntry[]>;
    }

    interface Props {
        pins: Pin[];
        drives: DriveInfo[];
        activePath: string;
        ipc: TreeIpc;
        onNavigate: (path: string) => void;
        onUnpin: (path: string) => void;
        onNavigateOther: (path: string) => void;
        /** Move dropped sources into a pin or tree-node path. */
        onMove?: (sources: string[], destination: string) => void;
    }

    const { pins, drives, activePath, ipc, onNavigate, onUnpin, onNavigateOther, onMove }: Props =
        $props();

    // The pin path currently under a valid drag, driving its droptarget outline.
    let dropTargetPin = $state<string | null>(null);

    /** Right-click a pin: open, open in the other pane, or unpin. */
    function pinMenu(event: MouseEvent, pin: Pin): void {
        openContextMenu(event, [
            { label: 'Open', onSelect: () => onNavigate(pin.path) },
            { label: 'Open in other pane', onSelect: () => onNavigateOther(pin.path) },
            { separator: true },
            { label: 'Unpin', danger: true, onSelect: () => onUnpin(pin.path) },
        ]);
    }

    function pinDragOver(event: DragEvent, pin: Pin): void {
        const sources = getDragSources();
        if (sources === null || !isValidDrop(sources, pin.path)) {
            return;
        }
        event.preventDefault();
        dropTargetPin = pin.path;
    }

    function pinDragLeave(pin: Pin): void {
        if (dropTargetPin === pin.path) {
            dropTargetPin = null;
        }
    }

    function pinDrop(event: DragEvent, pin: Pin): void {
        const sources = getDragSources();
        if (sources === null || !isValidDrop(sources, pin.path)) {
            return;
        }
        event.preventDefault();
        dropTargetPin = null;
        onMove?.(sources, pin.path);
        endDrag();
    }
</script>

<div class="sidebar">
    <div class="section">
        <div class="side-h">Pinned</div>
        {#each pins as pin (pin.path)}
            <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
            <div
                class="side-item"
                class:active={activePath === pin.path}
                class:droptarget={dropTargetPin === pin.path}
                onclick={() => onNavigate(pin.path)}
                oncontextmenu={(event) => pinMenu(event, pin)}
                ondragover={(event) => pinDragOver(event, pin)}
                ondragleave={() => pinDragLeave(pin)}
                ondrop={(event) => pinDrop(event, pin)}
                role="button"
                tabindex="-1"
            >
                <Icon name={pin.label === 'Home' ? 'home' : 'folder'} size={14} />
                <span class="nm">{pin.label}</span>
            </div>
        {/each}
    </div>

    <div class="section">
        <div class="side-h">Drives</div>
        {#each drives as drive (drive.mount_point)}
            {@const fraction = driveUsedFraction(drive)}
            {@const barClass = driveBarClass(fraction)}
            <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
            <div class="drive" onclick={() => onNavigate(drive.mount_point)} role="button" tabindex="-1">
                <div class="top">
                    <Icon name="drive" size={14} />
                    <span class="nm">{drive.label}</span>
                </div>
                <div class="gauge">
                    <i
                        class:warn={barClass === 'warn'}
                        class:crit={barClass === 'crit'}
                        style="width: {fraction * 100}%"
                    ></i>
                </div>
                <div class="free">{formatSize(drive.free_bytes)} free of {formatSize(drive.total_bytes)}</div>
            </div>
        {/each}
    </div>

    <div class="section">
        <div class="side-h">Tree</div>
        <DirectoryTree {ipc} {activePath} {onNavigate} {onMove} />
    </div>
</div>

<style>
    .sidebar {
        width: 210px;
        flex: none;
        box-sizing: border-box;
        border-right: 1px solid var(--color-border);
        overflow-y: auto;
        padding: 8px 6px;
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .side-h {
        font-size: 9px;
        text-transform: uppercase;
        letter-spacing: 0.12em;
        color: var(--color-muted);
        padding: 0 8px 4px;
    }
    .side-item {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 5px 8px;
        border-radius: 7px;
        font-size: 12px;
        color: var(--color-fg-alt);
        cursor: pointer;
        user-select: none;
    }
    .side-item:hover {
        background: var(--color-surface-hover, hsla(230, 14%, 42%, 1));
    }
    .side-item.active {
        background: color-mix(in oklab, var(--color-accent) 14%, transparent);
        color: var(--color-accent);
    }
    .side-item.droptarget {
        outline: 1px dashed var(--color-accent);
    }
    .side-item .nm {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        flex: 1;
    }
    .drive {
        padding: 6px 8px;
        border-radius: 7px;
        cursor: pointer;
    }
    .drive:hover {
        background: var(--color-surface-hover, hsla(230, 14%, 42%, 1));
    }
    .drive .top {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 12px;
        color: var(--color-fg-alt);
    }
    .drive .nm {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .gauge {
        margin-top: 5px;
        height: 4px;
        border-radius: 2px;
        background: var(--color-bg-alt);
        overflow: hidden;
    }
    .gauge i {
        display: block;
        height: 100%;
        border-radius: 2px;
        background: var(--color-accent);
    }
    .gauge i.warn {
        background: var(--color-warning);
    }
    .gauge i.crit {
        background: var(--color-error);
    }
    .free {
        margin-top: 3px;
        font-size: 9px;
        color: var(--color-muted);
    }
</style>
