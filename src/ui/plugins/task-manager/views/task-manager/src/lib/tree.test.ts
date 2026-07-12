import { describe, it, expect } from 'vitest';
import type { ProcessNode } from '@quantum/client';
import { sortNodes, flattenTree, glyphFor, fmtMem, type SortState } from './tree';

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
