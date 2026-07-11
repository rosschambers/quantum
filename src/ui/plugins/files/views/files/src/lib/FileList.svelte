<script lang="ts">
    /**
     * A virtualized, fixed-row-height file list. It renders only the rows that
     * fall within the scroll viewport (plus an overscan margin), so a directory
     * with tens of thousands of entries mounts a bounded handful of `Row`
     * components. A single full-height sizer keeps the native scrollbar honest.
     *
     * Virtualization is pure arithmetic on `scrollTop` and the viewport height;
     * there is no third-party dependency. The list is keyboard-agnostic — the
     * App owns keys (Task 24). Its empty background is a drop target that moves
     * a drag (typically from the other pane) into this pane's directory; the
     * rows themselves handle their own directory drops (Task 23).
     */
    import type { FileEntry } from '@quantum/client';
    import Row from './Row.svelte';
    import { getDragSources, endDrag } from './dragState.svelte';
    import { isValidDrop } from './dnd';

    interface Props {
        /** The already visible + sorted entries from `PaneState.visibleEntries()`. */
        entries: FileEntry[];
        /** Paths currently selected, used to highlight rows. */
        selection: Set<string>;
        /** The largest sibling size, forwarded to each row's mini usage bar. */
        maxSize: number;
        /**
         * This pane's current directory. The list background is a drop target
         * that moves a drag (typically from the other pane) into this path.
         */
        path?: string;
        /** Move dropped sources into this pane's directory. */
        onMove?: (sources: string[], destination: string) => void;
        /**
         * An explicit viewport height in pixels. When omitted the height is
         * measured from the scroll container via a bound `clientHeight`. Tests
         * inject a fixed value because jsdom does not lay out or measure
         * elements, so a measured height there is always zero.
         */
        viewportHeight?: number;
        onSelect: (path: string, event: MouseEvent) => void;
        onOpen: (entry: FileEntry) => void;
        onContextMenu: (entry: FileEntry, event: MouseEvent) => void;
    }

    const {
        entries,
        selection,
        maxSize,
        path,
        onMove,
        viewportHeight,
        onSelect,
        onOpen,
        onContextMenu,
    }: Props = $props();

    const ROW_HEIGHT = 30;
    const OVERSCAN = 10;

    let scrollTop = $state(0);
    // The scroll container element and its measured height. The height is
    // observed with a ResizeObserver in a `$effect`; the injected
    // `viewportHeight` prop overrides it when supplied.
    let container = $state<HTMLElement | null>(null);
    let measuredHeight = $state(0);

    const effectiveHeight = $derived(viewportHeight ?? measuredHeight);

    $effect(() => {
        if (container === null || typeof ResizeObserver === 'undefined') {
            return;
        }
        const element = container;
        measuredHeight = element.clientHeight;
        const observer = new ResizeObserver(() => {
            measuredHeight = element.clientHeight;
        });
        observer.observe(element);
        return () => observer.disconnect();
    });

    const startIndex = $derived(
        clamp(Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN, 0, entries.length),
    );
    const endIndex = $derived(
        clamp(
            startIndex + Math.ceil(effectiveHeight / ROW_HEIGHT) + 2 * OVERSCAN,
            0,
            entries.length,
        ),
    );

    const visible = $derived(entries.slice(startIndex, endIndex));
    const totalHeight = $derived(entries.length * ROW_HEIGHT);
    const offsetY = $derived(startIndex * ROW_HEIGHT);

    function clamp(value: number, low: number, high: number): number {
        return Math.max(low, Math.min(value, high));
    }

    function handleScroll(event: Event): void {
        scrollTop = (event.currentTarget as HTMLElement).scrollTop;
    }

    // The payload dragged from a selected row is the whole current selection;
    // Row only calls this when the dragged row is already part of the selection.
    function currentSelectionPaths(): string[] {
        return [...selection];
    }

    // True while a valid drag hovers anywhere in the pane, driving the outline.
    // The whole pane area is a drop target: a drop over empty space or over a
    // non-directory row bubbles here and moves into this pane's directory. A
    // directory `Row` stops propagation in its own drop handler, so a drop ON a
    // directory targets that directory and never reaches these handlers.
    let isListDropTarget = $state(false);

    function listAcceptsDrop(): boolean {
        if (path === undefined || onMove === undefined) {
            return false;
        }
        const sources = getDragSources();
        return sources !== null && isValidDrop(sources, path);
    }

    function handleListDragOver(event: DragEvent): void {
        if (!listAcceptsDrop()) {
            return;
        }
        event.preventDefault();
        isListDropTarget = true;
    }

    // Dragover fires for child elements too, so a naive dragleave would clear
    // the highlight every time the pointer crosses between children and flicker.
    // Clear only when the drag actually leaves the list: when `relatedTarget`
    // (the element being entered) is not contained by the scroll container.
    function handleListDragLeave(event: DragEvent): void {
        const list = event.currentTarget;
        const related = event.relatedTarget;
        if (list instanceof Node && related instanceof Node && list.contains(related)) {
            return;
        }
        isListDropTarget = false;
    }

    function handleListDrop(event: DragEvent): void {
        if (!listAcceptsDrop()) {
            return;
        }
        event.preventDefault();
        isListDropTarget = false;
        const sources = getDragSources();
        if (sources !== null && path !== undefined) {
            onMove?.(sources, path);
        }
        endDrag();
    }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
    class="list"
    class:droptarget={isListDropTarget}
    bind:this={container}
    onscroll={handleScroll}
    ondragover={handleListDragOver}
    ondragleave={handleListDragLeave}
    ondrop={handleListDrop}
>
    <div class="sizer" style="height: {totalHeight}px">
        <div class="rows" style="transform: translateY({offsetY}px)">
            {#each visible as entry (entry.path)}
                <Row
                    {entry}
                    {maxSize}
                    {onMove}
                    selected={selection.has(entry.path)}
                    dragSources={currentSelectionPaths}
                    onSelect={(event) => onSelect(entry.path, event)}
                    onOpen={() => onOpen(entry)}
                    onContextMenu={(event) => onContextMenu(entry, event)}
                />
            {/each}
        </div>
    </div>
</div>

<style>
    .list {
        flex: 1;
        overflow-y: auto;
        padding: 4px 6px;
        position: relative;
    }
    .list.droptarget {
        outline: 1px dashed var(--color-accent);
        outline-offset: -1px;
    }
    .sizer {
        position: relative;
        width: 100%;
    }
    .rows {
        position: absolute;
        top: 0;
        left: 0;
        right: 0;
        will-change: transform;
    }
</style>
