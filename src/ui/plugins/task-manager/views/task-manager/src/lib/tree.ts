// Pure helpers for the app-grouped process tree table. These are the sortable,
// flattenable core of the tree, kept free of Svelte so they can be unit-tested
// directly. Sorting and memory sizing use the backend-computed aggregate fields
// on `ProcessNode` (aggregate_cpu_percent, aggregate_mem_bytes) — the frontend
// never re-sums a subtree.

import type { ProcessNode } from '@quantum/client';

/** A column the tree can be sorted by. `name` sorts alphabetically. */
export type SortColumn = 'name' | 'cpu' | 'mem';

/** Which column the tree is sorted by, and in which direction. */
export interface SortState {
    column: SortColumn;
    descending: boolean;
}

/** Which glyph a row shows: an app root, a protected process, or a plain process. */
export type ProcessGlyph = 'app' | 'process' | 'self';

/**
 * One rendered row: the node, its depth in the tree (for indentation), whether
 * it has children (so the disclosure chevron shows), and whether it is currently
 * expanded. Rows carry their `node` — and therefore its `pid` — so a later task
 * can attach a right-click kill handler without reshaping this type.
 */
export interface RenderRow {
    node: ProcessNode;
    depth: number;
    hasChildren: boolean;
    expanded: boolean;
}

/**
 * Return a new list sorted by the given column and direction. Sorting is by the
 * aggregate values so a parent orders by the weight of its whole subtree; name
 * sorts alphabetically via `localeCompare`. Non-mutating: the input list is
 * left untouched.
 */
export function sortNodes(nodes: ProcessNode[], sort: SortState): ProcessNode[] {
    const sorted = [...nodes];
    sorted.sort((a, b) => {
        if (sort.column === 'name') {
            const order = a.name.localeCompare(b.name);
            return sort.descending ? -order : order;
        }
        const key = sort.column === 'cpu' ? 'aggregate_cpu_percent' : 'aggregate_mem_bytes';
        const order = a[key] - b[key];
        return sort.descending ? -order : order;
    });
    return sorted;
}

/**
 * Walk the forest into a flat list of render rows, honouring the expansion set:
 * a node's children are included only when its pid is in `expanded`. Each level
 * is sorted with the same sort state, so ordering applies all the way down.
 * Expansion is keyed by pid (not object identity), so a fresh snapshot with the
 * same pids keeps the user's expanded rows open.
 */
export function flattenTree(
    roots: ProcessNode[],
    expanded: Set<number>,
    sort: SortState,
): RenderRow[] {
    const rows: RenderRow[] = [];
    const visit = (nodes: ProcessNode[], depth: number): void => {
        for (const node of sortNodes(nodes, sort)) {
            const hasChildren = node.children.length > 0;
            const isExpanded = expanded.has(node.pid);
            rows.push({ node, depth, hasChildren, expanded: isExpanded });
            if (hasChildren && isExpanded) {
                visit(node.children, depth + 1);
            }
        }
    };
    visit(roots, 0);
    return rows;
}

/**
 * Choose the glyph for a row. Protected processes (quantumd itself) get the
 * distinct `self` glyph; top-level application entries get the `app` glyph;
 * everything else gets the plain `process` glyph.
 */
export function glyphFor(node: ProcessNode, isAppRoot: boolean): ProcessGlyph {
    if (node.protected) return 'self';
    return isAppRoot ? 'app' : 'process';
}

/**
 * Format a memory size given in megabytes: whole megabytes below a gigabyte
 * (`512 MB`), one decimal of gigabytes at and above (`1.0 GB`). The boundary is
 * 1024 MB, which reads as `1.0 GB`. Callers convert the backend's
 * `aggregate_mem_bytes` to megabytes before formatting.
 */
export function fmtMem(megabytes: number): string {
    if (megabytes >= 1024) {
        return `${(megabytes / 1024).toFixed(1)} GB`;
    }
    return `${Math.round(megabytes)} MB`;
}
