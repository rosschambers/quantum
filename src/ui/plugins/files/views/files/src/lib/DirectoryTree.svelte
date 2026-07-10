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

    interface TreeIpc {
        list(path: string): Promise<FileEntry[]>;
    }

    interface Props {
        ipc: TreeIpc;
        activePath: string;
        onNavigate: (path: string) => void;
    }

    const { ipc, activePath, onNavigate }: Props = $props();

    const ROOT = '/';

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
            const directories = entries.filter((entry) => entry.kind === 'directory');
            const next = new Map(loadedChildren);
            next.set(path, directories);
            loadedChildren = next;
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
        untrack(() => {
            for (const path of chain) {
                void expand(path);
            }
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
        data-path={path}
        style="padding-left: {4 + depth * 14}px"
        role="treeitem"
        aria-selected={activePath === path}
        aria-expanded={isOpen}
        tabindex="-1"
        onclick={() => onNavigate(path)}
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
    {#if isOpen && children}
        {#each children as child (child.path)}
            {@render treeNode(child.path, depth + 1)}
        {/each}
    {/if}
{/snippet}

<div class="tree" role="tree">
    {@render treeNode(ROOT, 0)}
</div>

<style>
    .tree {
        display: flex;
        flex-direction: column;
    }
    .tree-row {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 3px 4px;
        border-radius: 6px;
        font-size: 12px;
        color: var(--color-fg-alt);
        cursor: pointer;
        user-select: none;
        white-space: nowrap;
    }
    .tree-row:hover {
        background: var(--color-surface-hover);
    }
    .tree-row.active {
        background: rgba(166, 227, 161, 0.14);
        color: var(--color-accent);
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
