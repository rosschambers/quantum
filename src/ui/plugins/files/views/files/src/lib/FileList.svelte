<script lang="ts">
    /**
     * A virtualized, fixed-row-height file list. It renders only the rows that
     * fall within the scroll viewport (plus an overscan margin), so a directory
     * with tens of thousands of entries mounts a bounded handful of `Row`
     * components. A single full-height sizer keeps the native scrollbar honest.
     *
     * Virtualization is pure arithmetic on `scrollTop` and the viewport height;
     * there is no third-party dependency. The list is keyboard-agnostic — the
     * App owns keys (Task 24) — and drag-and-drop is deferred (Task 23). This
     * component only renders rows and reports pointer events upward.
     */
    import type { FileEntry } from '@quantum/client';
    import Row from './Row.svelte';

    interface Props {
        /** The already visible + sorted entries from `PaneState.visibleEntries()`. */
        entries: FileEntry[];
        /** Paths currently selected, used to highlight rows. */
        selection: Set<string>;
        /** The largest sibling size, forwarded to each row's mini usage bar. */
        maxSize: number;
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
</script>

<div class="list" bind:this={container} onscroll={handleScroll}>
    <div class="sizer" style="height: {totalHeight}px">
        <div class="rows" style="transform: translateY({offsetY}px)">
            {#each visible as entry (entry.path)}
                <Row
                    {entry}
                    {maxSize}
                    selected={selection.has(entry.path)}
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
