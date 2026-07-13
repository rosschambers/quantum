import { describe, it, expect } from 'vitest';
import type { ProcessNode } from '@quantum/client';
import {
    sortNodes,
    flattenTree,
    glyphFor,
    fmtMem,
    filterTree,
    splitHighlight,
    visibleWindow,
    type SortState,
} from './tree';

/** Build a `ProcessNode` with sensible defaults; aggregates default to self usage. */
function node(partial: Partial<ProcessNode> & { pid: number; name: string }): ProcessNode {
    const cpu = partial.cpu_percent ?? 0;
    const mem = partial.mem_bytes ?? 0;
    return {
        pid: partial.pid,
        name: partial.name,
        cpu_percent: cpu,
        mem_bytes: mem,
        aggregate_cpu_percent: partial.aggregate_cpu_percent ?? cpu,
        aggregate_mem_bytes: partial.aggregate_mem_bytes ?? mem,
        window: partial.window,
        protected: partial.protected ?? false,
        children: partial.children ?? [],
    };
}

const MB = 1024 * 1024;

describe('sortNodes', () => {
    const nodes: ProcessNode[] = [
        node({ pid: 1, name: 'bravo', aggregate_cpu_percent: 10, aggregate_mem_bytes: 300 * MB }),
        node({ pid: 2, name: 'alpha', aggregate_cpu_percent: 30, aggregate_mem_bytes: 100 * MB }),
        node({ pid: 3, name: 'charlie', aggregate_cpu_percent: 20, aggregate_mem_bytes: 200 * MB }),
    ];

    it('sorts by aggregate cpu descending', () => {
        const sort: SortState = { column: 'cpu', descending: true };
        expect(sortNodes(nodes, sort).map((n) => n.pid)).toEqual([2, 3, 1]);
    });

    it('sorts by aggregate cpu ascending', () => {
        const sort: SortState = { column: 'cpu', descending: false };
        expect(sortNodes(nodes, sort).map((n) => n.pid)).toEqual([1, 3, 2]);
    });

    it('sorts by aggregate memory descending', () => {
        const sort: SortState = { column: 'mem', descending: true };
        expect(sortNodes(nodes, sort).map((n) => n.pid)).toEqual([1, 3, 2]);
    });

    it('sorts by name alphabetically', () => {
        const sort: SortState = { column: 'name', descending: false };
        expect(sortNodes(nodes, sort).map((n) => n.name)).toEqual(['alpha', 'bravo', 'charlie']);
    });

    it('does not mutate the input list', () => {
        const original = nodes.map((n) => n.pid);
        sortNodes(nodes, { column: 'cpu', descending: true });
        expect(nodes.map((n) => n.pid)).toEqual(original);
    });
});

describe('flattenTree', () => {
    function buildForest(): ProcessNode[] {
        return [
            node({
                pid: 1,
                name: 'firefox',
                aggregate_cpu_percent: 12,
                children: [
                    node({ pid: 11, name: 'renderer', aggregate_cpu_percent: 6 }),
                    node({ pid: 12, name: 'gpu', aggregate_cpu_percent: 3 }),
                ],
            }),
            node({ pid: 2, name: 'kitty', aggregate_cpu_percent: 1 }),
        ];
    }

    const sort: SortState = { column: 'cpu', descending: true };

    it('hides children of a collapsed node', () => {
        const rows = flattenTree(buildForest(), new Set<number>(), sort);
        expect(rows.map((r) => r.node.pid)).toEqual([1, 2]);
        expect(rows[0].hasChildren).toBe(true);
        expect(rows[0].expanded).toBe(false);
    });

    it('reveals children of an expanded node, sorted within the level', () => {
        const rows = flattenTree(buildForest(), new Set<number>([1]), sort);
        expect(rows.map((r) => r.node.pid)).toEqual([1, 11, 12, 2]);
        expect(rows[1].depth).toBe(1);
    });

    it('keeps expansion across a fresh snapshot with the same pids', () => {
        const expanded = new Set<number>([1]);
        // First snapshot: expand firefox and confirm its children flatten.
        const first = flattenTree(buildForest(), expanded, sort);
        expect(first.map((r) => r.node.pid)).toEqual([1, 11, 12, 2]);

        // A brand-new snapshot object (fresh node instances, same pids) must
        // still flatten firefox as expanded — expansion is keyed by pid.
        const second = flattenTree(buildForest(), expanded, sort);
        const firefoxRow = second.find((r) => r.node.pid === 1);
        expect(firefoxRow?.expanded).toBe(true);
        expect(second.map((r) => r.node.pid)).toEqual([1, 11, 12, 2]);
    });
});

describe('glyphFor', () => {
    it('gives app roots the app glyph', () => {
        expect(glyphFor(node({ pid: 1, name: 'firefox' }), true)).toBe('app');
    });

    it('gives non-root processes the process glyph', () => {
        expect(glyphFor(node({ pid: 2, name: 'renderer' }), false)).toBe('process');
    });

    it('identifies the protected row with the self glyph regardless of position', () => {
        const guarded = node({ pid: 3, name: 'quantumd', protected: true });
        expect(glyphFor(guarded, false)).toBe('self');
        expect(glyphFor(guarded, true)).toBe('self');
    });
});

describe('filterTree', () => {
    function buildForest(): ProcessNode[] {
        return [
            node({
                pid: 1,
                name: 'firefox',
                window: { class: 'firefox', title: 'quantum design — Mozilla Firefox' },
                children: [
                    node({ pid: 11, name: 'Isolated Web Content' }),
                    node({ pid: 12, name: 'WebExtensions' }),
                ],
            }),
            node({
                pid: 2,
                name: 'kitty',
                children: [node({ pid: 21, name: 'zsh', children: [node({ pid: 22, name: 'nvim' })] })],
            }),
        ];
    }

    it('returns the roots unchanged for an empty or whitespace filter', () => {
        const forest = buildForest();
        expect(filterTree(forest, '')).toBe(forest);
        expect(filterTree(forest, '   ')).toBe(forest);
    });

    it('keeps ancestors of a deep match while pruning non-matching branches', () => {
        // "nvim" matches only pid 22, deep under kitty; firefox has no match.
        const filtered = filterTree(buildForest(), 'nvim');
        expect(filtered.map((n) => n.pid)).toEqual([2]);
        expect(filtered[0].children.map((n) => n.pid)).toEqual([21]);
        expect(filtered[0].children[0].children.map((n) => n.pid)).toEqual([22]);
    });

    it('matches on the window title even when the name does not', () => {
        // "mozilla" appears only in firefox's window title, not any process name.
        const filtered = filterTree(buildForest(), 'mozilla');
        expect(filtered.map((n) => n.pid)).toEqual([1]);
    });

    it('drops a matched node\'s non-matching children', () => {
        // "firefox" matches the root by name; its children match neither name
        // nor title, so they are pruned even though the parent survives.
        const filtered = filterTree(buildForest(), 'firefox');
        expect(filtered.map((n) => n.pid)).toEqual([1]);
        expect(filtered[0].children).toEqual([]);
    });

    it('excludes a whole subtree when nothing in it matches', () => {
        const filtered = filterTree(buildForest(), 'zzz-no-such-process');
        expect(filtered).toEqual([]);
    });

    it('does not mutate the input forest', () => {
        const forest = buildForest();
        filterTree(forest, 'nvim');
        expect(forest[0].children.map((n) => n.pid)).toEqual([11, 12]);
        expect(forest[1].children[0].children.map((n) => n.pid)).toEqual([22]);
    });
});

describe('flattenTree forceExpand', () => {
    const sort: SortState = { column: 'cpu', descending: true };

    it('reveals every level regardless of the expansion set when forced open', () => {
        const forest: ProcessNode[] = [
            node({ pid: 1, name: 'firefox', children: [node({ pid: 11, name: 'renderer' })] }),
        ];
        // Empty expansion set, but forceExpand shows the child anyway.
        const rows = flattenTree(forest, new Set<number>(), sort, true);
        expect(rows.map((r) => r.node.pid)).toEqual([1, 11]);
        expect(rows[0].expanded).toBe(true);
    });
});

describe('splitHighlight', () => {
    it('splits a string into before, match, and after around the first hit', () => {
        expect(splitHighlight('firefox', 'ref')).toEqual({
            before: 'fi',
            match: 'ref',
            after: 'ox',
        });
    });

    it('preserves the original casing of the matched slice', () => {
        expect(splitHighlight('Isolated Web Content', 'web')).toEqual({
            before: 'Isolated ',
            match: 'Web',
            after: ' Content',
        });
    });

    it('returns the whole string as before when the filter is empty', () => {
        expect(splitHighlight('kitty', '')).toEqual({ before: 'kitty', match: '', after: '' });
    });

    it('returns the whole string as before when there is no match', () => {
        expect(splitHighlight('kitty', 'zsh')).toEqual({ before: 'kitty', match: '', after: '' });
    });
});

describe('fmtMem', () => {
    it('formats whole megabytes below a gigabyte', () => {
        expect(fmtMem(512)).toBe('512 MB');
        expect(fmtMem(1023)).toBe('1023 MB');
    });

    it('rolls over to one decimal of gigabytes at 1024 MB', () => {
        expect(fmtMem(1024)).toBe('1.0 GB');
        expect(fmtMem(1536)).toBe('1.5 GB');
    });
});

describe('visibleWindow', () => {
    // A 100-item list of 25px rows in a 100px viewport (four rows visible).
    const COUNT = 100;
    const ROW = 25;
    const VIEWPORT = 100;

    it('returns an empty window with no padding for an empty list', () => {
        expect(visibleWindow(0, ROW, 0, VIEWPORT, 2)).toEqual({
            start: 0,
            end: 0,
            topPad: 0,
            bottomPad: 0,
        });
    });

    it('windows from the top with no overscan', () => {
        // Rows 0..3 are visible; the rest is one bottom spacer.
        expect(visibleWindow(COUNT, ROW, 0, VIEWPORT, 0)).toEqual({
            start: 0,
            end: 4,
            topPad: 0,
            bottomPad: (COUNT - 4) * ROW,
        });
    });

    it('applies overscan and clamps it at the top edge', () => {
        // Overscan cannot push start below zero.
        expect(visibleWindow(COUNT, ROW, 0, VIEWPORT, 2)).toEqual({
            start: 0,
            end: 6,
            topPad: 0,
            bottomPad: (COUNT - 6) * ROW,
        });
    });

    it('windows a middle scroll position with overscan on both sides', () => {
        // scrollTop 250 => first visible row 10, last visible row 14.
        expect(visibleWindow(COUNT, ROW, 250, VIEWPORT, 2)).toEqual({
            start: 8,
            end: 16,
            topPad: 8 * ROW,
            bottomPad: (COUNT - 16) * ROW,
        });
    });

    it('clamps the end and zeroes the bottom pad when scrolled to the end', () => {
        // Max scroll for 100 rows of 25px in a 100px viewport is 2400.
        expect(visibleWindow(COUNT, ROW, 2400, VIEWPORT, 2)).toEqual({
            start: 94,
            end: 100,
            topPad: 94 * ROW,
            bottomPad: 0,
        });
    });

    it('shows the whole list with no padding when the viewport is taller than the content', () => {
        expect(visibleWindow(3, ROW, 0, 1000, 2)).toEqual({
            start: 0,
            end: 3,
            topPad: 0,
            bottomPad: 0,
        });
    });

    it('degrades to the full list when the row height is not positive', () => {
        expect(visibleWindow(10, 0, 0, VIEWPORT, 2)).toEqual({
            start: 0,
            end: 10,
            topPad: 0,
            bottomPad: 0,
        });
    });
});
