<script lang="ts">
    import type { ProcessNode } from '@quantum/client';
    import Icon from './Icon.svelte';
    import {
        flattenTree,
        filterTree,
        splitHighlight,
        glyphFor,
        fmtMem,
        type SortState,
        type SortColumn,
        type RenderRow,
    } from './tree';

    /**
     * The app-grouped process tree. Two sections — Apps (roots expanded one
     * level by default) and Background (collapsed by default). The snapshot
     * arrives fresh every second, so expansion and sort state are held here in
     * `$state`, OUTSIDE the snapshot-derived flatten, keyed by pid; a new
     * snapshot with the same pids therefore keeps the user's expanded rows and
     * their chosen sort. Sorting and memory sizing use the backend aggregates.
     */
    interface Props {
        apps: ProcessNode[];
        background: ProcessNode[];
        /** Machine total memory, used to size the inline memory bars. */
        memTotalBytes: number;
        /**
         * Titlebar filter text. When non-empty, the tree is pruned to subtrees
         * containing a name/window-title match, both sections are forced open,
         * every surviving node is force-expanded so matches are visible, and the
         * matched substring is highlighted in the accent colour.
         */
        filterText?: string;
    }

    let { apps, background, memTotalBytes, filterText = '' }: Props = $props();

    // A non-empty (non-whitespace) filter switches the tree into filtered mode:
    // pruned rows, forced-open sections, force-expanded nodes, and highlights.
    const filtering = $derived(filterText.trim() !== '');

    const BYTES_PER_MB = 1024 * 1024;
    const INDENT_PER_LEVEL = 18;

    // Expansion keyed by pid, and the sort state. Both live outside the derived
    // rows so snapshot churn cannot reset them.
    let expanded = $state<Set<number>>(new Set());
    let sort = $state<SortState>({ column: 'cpu', descending: true });
    let appsOpen = $state(true);
    let backgroundOpen = $state(false);

    // Seed the default expansion once: app roots open one level. A plain (non-
    // reactive) flag guards it so a later collapse is not undone by the next
    // snapshot, and so this effect never reads the `expanded` state it writes.
    let seededApps = false;
    $effect(() => {
        const roots = apps;
        if (seededApps || roots.length === 0) return;
        seededApps = true;
        const next = new Set<number>();
        for (const root of roots) next.add(root.pid);
        expanded = next;
    });

    // While filtering, prune each forest to the matching subtrees and force the
    // sections open and every node expanded so matches are always visible; the
    // saved expansion and section-open state are ignored until the filter clears.
    const appRows = $derived(
        filtering
            ? flattenTree(filterTree(apps, filterText), expanded, sort, true)
            : appsOpen
              ? flattenTree(apps, expanded, sort)
              : [],
    );
    const backgroundRows = $derived(
        filtering
            ? flattenTree(filterTree(background, filterText), expanded, sort, true)
            : backgroundOpen
              ? flattenTree(background, expanded, sort)
              : [],
    );

    // With a filter active and nothing left in either section, show the empty
    // state instead of two bare section headers.
    const noMatches = $derived(filtering && appRows.length === 0 && backgroundRows.length === 0);

    function toggleExpanded(pid: number): void {
        const next = new Set(expanded);
        if (next.has(pid)) {
            next.delete(pid);
        } else {
            next.add(pid);
        }
        expanded = next;
    }

    function sortBy(column: SortColumn): void {
        if (sort.column === column) {
            sort = { column, descending: !sort.descending };
            return;
        }
        // Name defaults to ascending; the numeric columns default to descending
        // (heaviest first), matching the default CPU-descending sort.
        sort = { column, descending: column !== 'name' };
    }

    function cpuBarWidth(node: ProcessNode): number {
        return Math.min(100, Math.max(0, node.aggregate_cpu_percent));
    }

    function memBarWidth(node: ProcessNode): number {
        if (memTotalBytes <= 0) return 0;
        return Math.min(100, (node.aggregate_mem_bytes / memTotalBytes) * 100);
    }

    function glyphSize(glyph: string): number {
        return glyph === 'app' ? 15 : 13;
    }
</script>

<div class="treewrap">
    <table>
        <thead>
            <tr>
                <th class="name-col">
                    <button type="button" class="sorter" onclick={() => sortBy('name')}>
                        Name
                        {#if sort.column === 'name'}
                            <span class="arrow">{sort.descending ? '▼' : '▲'}</span>
                        {/if}
                    </button>
                </th>
                <th class="num pid-col">PID</th>
                <th class="num">
                    <button type="button" class="sorter num" onclick={() => sortBy('cpu')}>
                        CPU %
                        {#if sort.column === 'cpu'}
                            <span class="arrow">{sort.descending ? '▼' : '▲'}</span>
                        {/if}
                    </button>
                </th>
                <th class="num">
                    <button type="button" class="sorter num" onclick={() => sortBy('mem')}>
                        Memory
                        {#if sort.column === 'mem'}
                            <span class="arrow">{sort.descending ? '▼' : '▲'}</span>
                        {/if}
                    </button>
                </th>
            </tr>
        </thead>
        <tbody>
            {#if noMatches}
                <tr class="empty">
                    <td colspan="4">No processes match "{filterText.trim()}"</td>
                </tr>
            {:else}
                <tr class="section">
                    <td colspan="4">
                        <button
                            type="button"
                            class="section-toggle"
                            onclick={() => (appsOpen = !appsOpen)}
                        >
                            {appsOpen || filtering ? '▾' : '▸'} Apps
                        </button>
                    </td>
                </tr>
                {#if appsOpen || filtering}
                    {#each appRows as row (row.node.pid)}
                        {@render processRow(row, true)}
                    {/each}
                {/if}

                <tr class="section">
                    <td colspan="4">
                        <button
                            type="button"
                            class="section-toggle"
                            onclick={() => (backgroundOpen = !backgroundOpen)}
                        >
                            {backgroundOpen || filtering ? '▾' : '▸'} Background
                        </button>
                    </td>
                </tr>
                {#if backgroundOpen || filtering}
                    {#each backgroundRows as row (row.node.pid)}
                        {@render processRow(row, false)}
                    {/each}
                {/if}
            {/if}
        </tbody>
    </table>
</div>

{#snippet highlighted(text: string)}{@const parts = splitHighlight(text, filterText)}{parts.before}{#if parts.match}<mark>{parts.match}</mark>{/if}{parts.after}{/snippet}

{#snippet processRow(row: RenderRow, isApps: boolean)}
    {@const node = row.node}
    {@const isAppRoot = isApps && row.depth === 0}
    {@const glyph = glyphFor(node, isAppRoot)}
    <tr data-pid={node.pid}>
        <td>
            <div class="name" style="padding-left: {row.depth * INDENT_PER_LEVEL}px">
                <button
                    type="button"
                    class="chev"
                    class:open={row.expanded}
                    class:leaf={!row.hasChildren}
                    aria-label={row.expanded ? 'Collapse' : 'Expand'}
                    tabindex={row.hasChildren ? 0 : -1}
                    onclick={() => toggleExpanded(node.pid)}
                >▶</button>
                <span class="glyph {glyph}">
                    <Icon name={glyph} size={glyphSize(glyph)} />
                </span>
                <span class="pname">{@render highlighted(node.name)}</span>
                {#if node.protected}
                    <span class="wintitle">(protected)</span>
                {/if}
                {#if isAppRoot && node.window}
                    <span class="wintitle">— {@render highlighted(node.window.title)}</span>
                {/if}
            </div>
        </td>
        <td class="num pid-col"><span>{node.pid}</span></td>
        <td class="num">
            <i class="cellbar" style="width: {cpuBarWidth(node)}%"></i>
            <span>{node.aggregate_cpu_percent.toFixed(1)}</span>
        </td>
        <td class="num">
            <i class="cellbar" style="width: {memBarWidth(node)}%"></i>
            <span>{fmtMem(node.aggregate_mem_bytes / BYTES_PER_MB)}</span>
        </td>
    </tr>
{/snippet}

<style>
    .treewrap {
        flex: 1;
        overflow-y: auto;
        padding: 0 14px 12px;
    }
    table {
        width: 100%;
        border-collapse: collapse;
        font-size: 13px;
    }
    thead th {
        position: sticky;
        top: 0;
        background: var(--color-bg, #292520);
        z-index: 2;
        text-align: left;
        padding: 0;
        border-bottom: 1px solid var(--color-border, #4a443e);
    }
    thead th.num {
        text-align: right;
    }
    .sorter {
        display: inline-flex;
        align-items: baseline;
        gap: 4px;
        width: 100%;
        background: none;
        border: 0;
        cursor: pointer;
        user-select: none;
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted, #8a8578);
        font-weight: 600;
        font-family: inherit;
        padding: 8px 8px 6px;
    }
    .sorter.num {
        justify-content: flex-end;
    }
    .sorter:hover {
        color: var(--color-fg, #e8e4d8);
    }
    .arrow {
        font-size: 9px;
    }
    tbody td {
        padding: 5px 8px;
        border-bottom: 1px solid var(--color-divider, rgba(232, 228, 216, 0.25));
        white-space: nowrap;
    }
    tbody tr:hover td {
        background: var(--color-surface-hover, #4a443e);
    }
    tr.section td {
        background: transparent;
        padding: 12px 8px 4px;
    }
    tr.section:hover td {
        background: transparent;
    }
    tr.empty td {
        color: var(--color-muted, #8a8578);
        text-align: center;
        padding: 24px;
    }
    tr.empty:hover td {
        background: transparent;
    }
    /* The matched substring of a filtered row: no background fill, just the
       accent colour and a touch more weight, matching the design playground. */
    mark {
        background: none;
        color: var(--color-accent, #a6e3a1);
        font-weight: 600;
    }
    .section-toggle {
        background: none;
        border: 0;
        cursor: pointer;
        color: var(--color-muted, #8a8578);
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        font-weight: 600;
        font-family: inherit;
        padding: 0;
    }
    td.num {
        text-align: right;
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 12px;
        font-variant-numeric: tabular-nums;
        position: relative;
    }
    td.num span {
        position: relative;
        z-index: 1;
    }
    .name {
        display: flex;
        align-items: center;
        gap: 6px;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .pname {
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .wintitle {
        color: var(--color-muted, #8a8578);
        font-size: 12px;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .chev {
        width: 14px;
        height: 14px;
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: none;
        border: 0;
        padding: 0;
        font-size: 9px;
        color: var(--color-muted, #8a8578);
        cursor: pointer;
        transition: transform 0.12s;
    }
    .chev.open {
        transform: rotate(90deg);
    }
    .chev.leaf {
        visibility: hidden;
    }
    .glyph {
        flex: none;
        display: inline-flex;
        line-height: 0;
    }
    .glyph.app {
        color: var(--color-accent, #a6e3a1);
    }
    .glyph.process,
    .glyph.self {
        color: var(--color-muted, #8a8578);
    }
    /* Inline usage bar behind the numeric cells: a theme-accent gradient that
       fades from a faint accent on the left to full accent on the right —
       distinct from the hot/cold graph strokes. `color-mix` keeps the faint
       end token-based rather than a hardcoded rgba. */
    .cellbar {
        position: absolute;
        right: 0;
        top: 15%;
        height: 70%;
        border-radius: 3px;
        opacity: 0.28;
        z-index: 0;
        pointer-events: none;
        background: linear-gradient(
            90deg,
            color-mix(in srgb, var(--color-accent, #a6e3a1) 15%, transparent),
            var(--color-accent, #a6e3a1)
        );
    }
</style>
