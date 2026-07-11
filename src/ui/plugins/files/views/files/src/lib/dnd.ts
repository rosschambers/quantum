// Pure drag-and-drop guard, kept in a plain module (not `.svelte.ts`) so it
// carries no reactive state and is trivially unit-testable. A move is only
// valid when the destination directory is neither one of the dragged sources,
// nor a descendant of one (which would mean dropping a folder into itself),
// nor the directory a source already lives in (a no-op the backend rejects).

import { parentOf } from './path';

/**
 * Whether moving `sources` into `destination` is a legal move. It is illegal
 * when there is nothing to move, when the destination IS one of the sources,
 * when the destination sits inside one of the sources (a folder cannot be
 * moved into its own subtree), or when the destination is the direct parent of
 * a source (the source already lives there, so the move is a no-op the backend
 * rejects with AlreadyExists). Only a true path boundary counts, so "/a" is not
 * treated as an ancestor of the sibling "/ab".
 */
export function isValidDrop(sources: string[], destination: string): boolean {
    if (sources.length === 0) {
        return false;
    }
    for (const source of sources) {
        if (
            destination === source ||
            destination.startsWith(`${source}/`) ||
            destination === parentOf(source)
        ) {
            return false;
        }
    }
    return true;
}
