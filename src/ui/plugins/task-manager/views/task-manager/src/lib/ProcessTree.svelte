<script lang="ts">
    import { untrack } from 'svelte';
    import {
        openContextMenu,
        PROCESSES_KILL,
        type Client,
        type KillSignal,
        type ProcessNode,
    } from '@quantum/client';
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
    import {
        buildKillMenuItems,
        collectSubtreePids,
        collectPids,
        reconcileDying,
        killErrorMessage,
    } from './kill';

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
        /**
         * IPC client, injected so tests can pass a stub. Used to signal a
         * process subtree via `processes.kill` from the right-click menu.
         */
        client: Client;
    }

    let { apps, background, memTotalBytes, filterText = '', client }: Props = $props();

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

    // Pids of subtrees a kill was issued for. Marked optimistically the instant
    // the menu action fires, so the whole subtree fades at once; a killed
    // process then vanishes naturally when the next snapshot no longer lists it.
    let dying = $state<Set<number>>(new Set());
    // The last kill error, shown in a dismissible strip above the table; null
    // when there is nothing to show. Cleared by the next successful kill.
    let killError = $state<string | null>(null);

    // Reconcile the dying set against every fresh snapshot: drop any faded pid
    // the snapshot no longer contains (it actually exited). `apps`/`background`
    // are the reactive triggers; `dying` is read via `untrack` so writing it
    // here cannot re-trigger this effect. reconcile only ever shrinks the set,
    // so a size change is a sufficient, loop-free signal that a write is needed.
    $effect(() => {
        const present = collectPids([...apps, ...background]);
        untrack(() => {
            const next = reconcileDying(dying, present);
            if (next.size !== dying.size) {
                dying = next;
            }
        });
    });

    /**
     * Open the kill context menu for a right-clicked row. A protected row gets a
     * single disabled notice; every other row gets End / Force kill wired to
     * `killSubtree`. The shared `openContextMenu` runtime owns placement,
     * dismissal, and theming (it suppresses the default browser menu itself).
     */
    function onRowContextMenu(event: MouseEvent, node: ProcessNode): void {
        openContextMenu(
            event,
            buildKillMenuItems(node, (signal) => killSubtree(node, signal)),
        );
    }

    /**
     * Signal a process subtree. The whole subtree is marked dying immediately
     * (optimistic fade); on success the error strip is cleared, on failure the
     * optimistic fade is rolled back (the processes did not die) and the
     * rejection message is surfaced in the strip.
     */
    function killSubtree(node: ProcessNode, signal: KillSignal): void {
        const pids = collectSubtreePids(node);
        dying = new Set([...dying, ...pids]);
        client
            .call(PROCESSES_KILL, { pid: node.pid, signal })
            .then(() => {
                killError = null;
            })
            .catch((error: unknown) => {
                const reverted = new Set(dying);
                for (const pid of pids) {
                    reverted.delete(pid);
                }
                dying = reverted;
                killError = killErrorMessage(error);
            });
    }

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
    {#if killError}
        <div class="kill-error" role="alert">
            <span class="kill-error-message">{killError}</span>
            <button
                type="button"
                class="kill-error-dismiss"
                aria-label="Dismiss error"
                onclick={() => (killError = null)}
            >
                <Icon name="close" size={12} />
            </button>
        </div>
    {/if}
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
    <tr
        data-pid={node.pid}
        class:dying={dying.has(node.pid)}
        oncontextmenu={(event) => onRowContextMenu(event, node)}
    >
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
    /* Inline error strip shown above the table when a kill call rejects. Uses
       the theme destructive token; a small X dismisses it. */
    .kill-error {
        position: sticky;
        top: 0;
        z-index: 3;
        display: flex;
        align-items: center;
        gap: 8px;
        margin: 8px 0;
        padding: 6px 8px 6px 10px;
        border-radius: 8px;
        background: color-mix(in srgb, var(--color-error, #af4e3c) 18%, transparent);
        border: 1px solid var(--color-error, #af4e3c);
        color: var(--color-fg, #e8e4d8);
        font-size: 12.5px;
    }
    .kill-error-message {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .kill-error-dismiss {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 20px;
        height: 20px;
        padding: 0;
        border: 0;
        border-radius: 6px;
        background: none;
        color: var(--color-fg-alt, #d8d4c8);
        cursor: pointer;
    }
    .kill-error-dismiss:hover {
        background: var(--color-error, #af4e3c);
        color: var(--color-fg, #e8e4d8);
    }
    /* A killed subtree fades until the next snapshot drops it. */
    tr.dying td {
        opacity: 0.35;
        transition: opacity 0.4s;
    }
    table {
        width: 100%;
        border-collapse: collapse;
        font-size: 13px;
        /* Fixed layout so a long app name or window title cannot stretch the
           Name column and shove the numeric columns off a narrow (partial-
           width) window. Column widths below are honoured instead of content. */
        table-layout: fixed;
    }
    /* Cap the numeric columns at tight fixed widths so the Name column (no
       explicit width) takes only the remaining space and its contents truncate
       with an ellipsis rather than widening the table on a narrow window. */
    .pid-col {
        width: 64px;
    }
    thead th.num:not(.pid-col) {
        width: 92px;
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
        /* min-width: 0 lets this flex container actually shrink inside the
           fixed-width cell so its children can ellipsize. */
        min-width: 0;
    }
    .pname {
        /* min-width: 0 is required for text-overflow to trigger on a flex item.
           The process name keeps its natural width but will truncate before it
           overflows the cell. */
        min-width: 0;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        flex: 0 1 auto;
    }
    .wintitle {
        color: var(--color-muted, #8a8578);
        font-size: 12px;
        min-width: 0;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        /* The window title absorbs the remaining space and is the first thing
           to truncate as the window narrows. */
        flex: 1 1 auto;
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
