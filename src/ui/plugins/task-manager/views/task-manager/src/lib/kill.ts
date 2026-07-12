// Pure helpers for the right-click subtree kill flow, kept free of Svelte so
// they can be unit-tested directly. The component (`ProcessTree.svelte`) owns
// the `$state` (the dying-pid set and the error strip) and the IPC call; these
// functions build the menu, enumerate the pids a kill touches, reconcile the
// dying set against a fresh snapshot, and normalise a rejection into a message.

import type { KillSignal, MenuItem, ProcessNode } from '@quantum/client';

/** Leading glyphs for the two kill actions. Plain text, never emoji. */
const END_GLYPH = '\u25A0'; // ■
const FORCE_KILL_GLYPH = '\u2715'; // ✕

/**
 * Build the context-menu items for a right-clicked row. A protected process
 * (quantumd itself) offers no kill actions — just a single disabled notice.
 * Every other process offers "End" (SIGTERM the subtree) and, below a divider,
 * "Force kill" (SIGKILL the subtree, styled destructive). Selecting either
 * invokes `onKill` with the matching signal, so the caller owns the IPC call
 * and the tests can assert the signal without mounting anything.
 */
export function buildKillMenuItems(
    node: Pick<ProcessNode, 'protected'>,
    onKill: (signal: KillSignal) => void,
): MenuItem[] {
    if (node.protected) {
        return [{ label: 'quantumd is protected', disabled: true }];
    }
    return [
        { label: 'End', icon: END_GLYPH, onSelect: () => onKill('term') },
        { separator: true, label: '' },
        { label: 'Force kill', icon: FORCE_KILL_GLYPH, danger: true, onSelect: () => onKill('kill') },
    ];
}

/**
 * Every pid in a node's subtree, itself first. When a kill is issued the whole
 * subtree is optimistically marked dying, so the parent and all its descendants
 * fade together until the next snapshot drops the ones that actually died.
 */
export function collectSubtreePids(node: ProcessNode): number[] {
    const pids: number[] = [node.pid];
    for (const child of node.children) {
        pids.push(...collectSubtreePids(child));
    }
    return pids;
}

/** Every pid present anywhere in a forest, used to reconcile the dying set. */
export function collectPids(roots: ProcessNode[]): Set<number> {
    const pids = new Set<number>();
    const visit = (nodes: ProcessNode[]): void => {
        for (const node of nodes) {
            pids.add(node.pid);
            visit(node.children);
        }
    };
    visit(roots);
    return pids;
}

/**
 * Drop from the dying set any pid no longer present in the latest snapshot: a
 * faded process that has actually exited disappears naturally instead of
 * lingering. Returns a new set (never mutates the input) containing only the
 * dying pids that are still present.
 */
export function reconcileDying(dying: Set<number>, present: Set<number>): Set<number> {
    const next = new Set<number>();
    for (const pid of dying) {
        if (present.has(pid)) {
            next.add(pid);
        }
    }
    return next;
}

/**
 * Turn a rejected kill call into a human-readable strip message. The client
 * rejects with a `ClientError` (`{ code, message }`); fall back through a plain
 * `Error`, a bare string, and finally a generic message so the strip always has
 * something to show.
 */
export function killErrorMessage(error: unknown): string {
    if (error && typeof error === 'object' && 'message' in error) {
        const message = (error as { message: unknown }).message;
        if (typeof message === 'string' && message.trim() !== '') {
            return message;
        }
    }
    if (typeof error === 'string' && error.trim() !== '') {
        return error;
    }
    return 'Failed to end process';
}
