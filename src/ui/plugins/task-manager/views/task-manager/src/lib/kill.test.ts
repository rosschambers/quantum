import { describe, it, expect, vi } from 'vitest';
import type { ProcessNode } from '@quantum/client';
import {
    buildKillMenuItems,
    collectSubtreePids,
    collectPids,
    reconcileDying,
    killErrorMessage,
} from './kill';

/** Build a `ProcessNode` with sensible defaults for the kill helpers. */
function node(partial: Partial<ProcessNode> & { pid: number; name: string }): ProcessNode {
    return {
        pid: partial.pid,
        name: partial.name,
        cpu_percent: 0,
        mem_bytes: 0,
        aggregate_cpu_percent: 0,
        aggregate_mem_bytes: 0,
        window: partial.window,
        protected: partial.protected ?? false,
        children: partial.children ?? [],
    };
}

describe('buildKillMenuItems', () => {
    it('offers End (SIGTERM) and Force kill (SIGKILL) for a normal process', () => {
        const onKill = vi.fn();
        const items = buildKillMenuItems(node({ pid: 1, name: 'firefox' }), onKill);

        // End, a divider, then a destructive Force kill.
        expect(items.map((item) => item.label)).toEqual(['End', '', 'Force kill']);
        expect(items[1].separator).toBe(true);
        expect(items[2].danger).toBe(true);
        expect(items[0].disabled).toBeFalsy();

        items[0].onSelect?.();
        expect(onKill).toHaveBeenLastCalledWith('term');
        items[2].onSelect?.();
        expect(onKill).toHaveBeenLastCalledWith('kill');
    });

    it('offers only a single disabled notice for a protected process', () => {
        const onKill = vi.fn();
        const items = buildKillMenuItems(
            node({ pid: 2, name: 'quantumd', protected: true }),
            onKill,
        );

        expect(items).toHaveLength(1);
        expect(items[0].label).toBe('quantumd is protected');
        expect(items[0].disabled).toBe(true);
        expect(items[0].onSelect).toBeUndefined();
    });
});

describe('collectSubtreePids', () => {
    it('returns the node pid first, then every descendant', () => {
        const tree = node({
            pid: 1,
            name: 'firefox',
            children: [
                node({ pid: 11, name: 'renderer', children: [node({ pid: 111, name: 'gpu' })] }),
                node({ pid: 12, name: 'extension' }),
            ],
        });
        expect(collectSubtreePids(tree)).toEqual([1, 11, 111, 12]);
    });
});

describe('collectPids', () => {
    it('gathers every pid across a forest', () => {
        const forest = [
            node({ pid: 1, name: 'a', children: [node({ pid: 11, name: 'a1' })] }),
            node({ pid: 2, name: 'b' }),
        ];
        expect(collectPids(forest)).toEqual(new Set([1, 11, 2]));
    });
});

describe('reconcileDying', () => {
    it('keeps dying pids that are still present', () => {
        const result = reconcileDying(new Set([1, 2, 3]), new Set([1, 2, 3, 4]));
        expect(result).toEqual(new Set([1, 2, 3]));
    });

    it('drops a dying pid once it is absent from the new snapshot', () => {
        const result = reconcileDying(new Set([1, 2, 3]), new Set([1, 3]));
        expect(result).toEqual(new Set([1, 3]));
    });

    it('does not mutate the input set', () => {
        const dying = new Set([1, 2]);
        reconcileDying(dying, new Set([1]));
        expect(dying).toEqual(new Set([1, 2]));
    });
});

describe('killErrorMessage', () => {
    it('uses the message from a ClientError-shaped rejection', () => {
        expect(killErrorMessage({ code: -32000, message: 'No such process' })).toBe(
            'No such process',
        );
    });

    it('uses the message from a plain Error', () => {
        expect(killErrorMessage(new Error('permission denied'))).toBe('permission denied');
    });

    it('uses a bare string rejection directly', () => {
        expect(killErrorMessage('boom')).toBe('boom');
    });

    it('falls back to a generic message for an empty or unknown rejection', () => {
        expect(killErrorMessage({ code: 1, message: '' })).toBe('Failed to end process');
        expect(killErrorMessage(undefined)).toBe('Failed to end process');
        expect(killErrorMessage(null)).toBe('Failed to end process');
    });
});
