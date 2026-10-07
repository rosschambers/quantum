<script lang="ts">
    /**
     * A lazily-loaded directory tree for the sidebar. It renders DIRECTORIES
     * only, rooted at the filesystem root "/". A node's children are fetched
     * via `ipc.list(path)` only the first time it is expanded and then cached,
     * so collapsing and re-expanding never re-fetches. Dotfile directories are
     * always shown (hidden files are never filtered here). Clicking a row body
     * navigates; clicking the chevron expands or collapses. When `activePath`
     * changes the ancestor chain is auto-expanded so the active node is visible.
     */
    import { untrack } from 'svelte';
    import type { FileEntry } from '@quantum/client';
    import Icon from './Icon.svelte';
    import { ancestorPaths, pathBaseName } from './path';
    import { getDragSources, endDrag } from './dragState.svelte';
    import { isValidDrop } from './dnd';

    interface TreeIpc {
        list(path: string): Promise<FileEntry[]>;
    }

    interface Props {
        ipc: TreeIpc;
        activePath: string;
        onNavigate: (path: string) => void;
        /** Move dropped sources into a tree node's directory. */
        onMove?: (sources: string[], destination: string) => void;
    }

    const { ipc, activePath, onNavigate, onMove }: Props = $props();

    const ROOT = '/';
    const ROW_HEIGHT = 20;
    const OVERSCAN = 10;
    let tree = $state<HTMLElement | null>(null);
    let scrollContainer: HTMLElement | null = null;
    let scrollTop = $state(0);
    let viewportHeight = $state(0);
    let revealPath = $state<string | null>(null);
    let disposed = false;

    // The node path currently under a valid drag, driving its droptarget outline.
    let dropTargetPath = $state<string | null>(null);

    function nodeDragOver(event: DragEvent, path: string): void {
        const sources = getDragSources();
        if (sources === null || !isValidDrop(sources, path)) {
            return;
        }
        event.preventDefault();
        dropTargetPath = path;
    }

    function nodeDragLeave(path: string): void {
        if (dropTargetPath === path) {
            dropTargetPath = null;
        }
    }

    function nodeDrop(event: DragEvent, path: string): void {
        const sources = getDragSources();
        if (sources === null || !isValidDrop(sources, path)) {
            return;
        }
        event.preventDefault();
        dropTargetPath = null;
        onMove?.(sources, path);
        endDrag();
    }

    // Loaded directory children keyed by path. Presence in the map means "this
    // path has been listed"; the array is the directory-only children. Reassigned
    // (not mutated in place) so the Svelte 5 runes see the change.
    let loadedChildren = $state(new Map<string, FileEntry[]>());
    // Currently expanded paths.
    let expanded = $state(new Set<string>([ROOT]));
    // In-flight list() calls, so a path is never fetched twice concurrently.
    const pending = new Set<string>();

    /** Fetch and cache a path's directory children once. */
    async function ensureLoaded(path: string): Promise<void> {
        if (loadedChildren.has(path) || pending.has(path)) {
            return;
        }
        pending.add(path);
        try {
            const entries = await ipc.list(path);
            if (disposed) return;
            const directories = entries.filter((entry) => entry.kind === 'directory');
            const next = new Map(loadedChildren);
            next.set(path, directories);
            loadedChildren = next;
        } catch {
            // Leave failures uncached so a later expansion can retry.
        } finally {
            pending.delete(path);
        }
    }

    /** Expand a node (marking it open and loading its children if needed). */
    async function expand(path: string): Promise<void> {
        if (!expanded.has(path)) {
            const next = new Set(expanded);
            next.add(path);
            expanded = next;
        }
        await ensureLoaded(path);
    }

    /** Collapse a node without discarding its cached children. */
    function collapse(path: string): void {
        if (expanded.has(path)) {
            const next = new Set(expanded);
            next.delete(path);
            expanded = next;
        }
    }

    /** Chevron click: toggle open state. Never navigates. */
    function toggle(event: MouseEvent, path: string): void {
        event.stopPropagation();
        if (expanded.has(path)) {
            collapse(path);
        } else {
            void expand(path);
        }
    }

    // Auto-expand the ancestor chain leading to the active path so it is visible.
    // `untrack` keeps this effect depending only on `activePath`, not on the
    // expansion state it mutates, so idempotent re-expansion cannot loop.
    $effect(() => {
        const chain = ancestorPaths(activePath);
        revealPath = activePath;
        untrack(() => {
            for (const path of chain) {
                void expand(path);
            }
        });
    });

    // Flatten only expanded, already-loaded nodes once per tree change, not per scroll.
    const rows = $derived.by(() => {
        const result: Array<{ path: string; depth: number }> = [];
        const stack = [{ path: ROOT, depth: 0 }];
        while (stack.length > 0) {
            const node = stack.pop()!;
            result.push(node);
            if (expanded.has(node.path)) {
                const children = loadedChildren.get(node.path) ?? [];
                for (let index = children.length - 1; index >= 0; index -= 1) {
                    stack.push({ path: children[index].path, depth: node.depth + 1 });
                }
            }
        }
        return result;
    });
    const startIndex = $derived(Math.max(0, Math.min(rows.length, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN)));
    const visibleRows = $derived(rows.slice(startIndex, startIndex + Math.ceil(viewportHeight / ROW_HEIGHT) + 2 * OVERSCAN));

    function treeOffset(): number {
        if (tree === null || scrollContainer === null || tree === scrollContainer) return 0;
        return tree.getBoundingClientRect().top - scrollContainer.getBoundingClientRect().top + scrollContainer.scrollTop;
    }

    function measureViewport(): void {
        if (scrollContainer === null) return;
        const offset = treeOffset();
        scrollTop = Math.max(0, scrollContainer.scrollTop - offset);
        viewportHeight = Math.max(0, scrollContainer.clientHeight - Math.max(0, offset - scrollContainer.scrollTop));
    }

    $effect(() => {
        if (tree === null) return;
        // The sidebar already owns scrolling; keep pins, drives and the tree in that viewport.
        const element = tree.closest<HTMLElement>('.sidebar') ?? tree;
        scrollContainer = element;
        untrack(measureViewport);
        element.addEventListener('scroll', measureViewport);
        const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measureViewport);
        observer?.observe(element);
        observer?.observe(tree);
        return () => {
            disposed = true;
            element.removeEventListener('scroll', measureViewport);
            observer?.disconnect();
            scrollContainer = null;
        };
    });

    $effect(() => {
        if (tree === null || revealPath === null) return;
        const index = rows.findIndex((row) => row.path === revealPath);
        if (index < 0) return;
        untrack(() => {
            if (scrollContainer === null) return;
            const top = treeOffset() + index * ROW_HEIGHT;
            const bottom = top + ROW_HEIGHT;
            if (top < scrollContainer.scrollTop) scrollContainer.scrollTop = top;
            else if (bottom > scrollContainer.scrollTop + scrollContainer.clientHeight) {
                scrollContainer.scrollTop = Math.max(0, bottom - scrollContainer.clientHeight);
            }
            measureViewport();
            revealPath = null;
        });
    });
</script>

{#snippet treeNode(path: string, depth: number)}
    {@const children = loadedChildren.get(path)}
    {@const isOpen = expanded.has(path)}
    {@const isLeaf = children !== undefined && children.length === 0}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div
        class="tree-row"
        class:active={activePath === path}
        class:droptarget={dropTargetPath === path}
        data-path={path}
        style="padding-left: {4 + depth * 14}px; height: {ROW_HEIGHT}px"
        role="treeitem"
        aria-selected={activePath === path}
        aria-expanded={isOpen}
        aria-level={depth + 1}
        tabindex="-1"
        onclick={() => onNavigate(path)}
        ondragover={(event) => nodeDragOver(event, path)}
        ondragleave={() => nodeDragLeave(path)}
        ondrop={(event) => nodeDrop(event, path)}
    >
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <span
            class="chev"
            class:open={isOpen}
            class:leaf={isLeaf}
            role="button"
            tabindex="-1"
            aria-label={isOpen ? 'Collapse' : 'Expand'}
            onclick={(event) => toggle(event, path)}
        >
            <Icon name="chevron" size={9} />
        </span>
        <span class="fico"><Icon name="folder" size={13} /></span>
        <span class="nm">{pathBaseName(path)}</span>
    </div>
{/snippet}

<div class="tree" role="tree" bind:this={tree} style="height: {rows.length * ROW_HEIGHT}px">
    <div class="tree-rows" style="transform: translateY({startIndex * ROW_HEIGHT}px)">
        {#each visibleRows as node (node.path)}
            {@render treeNode(node.path, node.depth)}
        {/each}
    </div>
</div>

<style>
    .tree {
        position: relative;
    }
    .tree-rows {
        position: absolute;
        top: 0;
        left: 0;
        right: 0;
    }
    .tree-row {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 3px 4px;
        box-sizing: border-box;
        border-radius: 6px;
        font-size: 12px;
        color: var(--color-fg-alt);
        cursor: pointer;
        user-select: none;
        white-space: nowrap;
    }
    .tree-row:hover {
        background: var(--color-surface-hover, hsla(230, 14%, 42%, 1));
    }
    .tree-row.active {
        background: color-mix(in oklab, var(--color-accent) 14%, transparent);
        color: var(--color-accent);
    }
    .tree-row.droptarget {
        outline: 1px dashed var(--color-accent);
    }
    .chev {
        width: 14px;
        height: 14px;
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        color: var(--color-muted);
        transition: transform 0.12s;
    }
    .chev.open {
        transform: rotate(90deg);
    }
    .chev.leaf {
        visibility: hidden;
    }
    .fico {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        opacity: 0.8;
    }
    .nm {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
</style>
