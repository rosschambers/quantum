// Pure drag-and-drop guard, kept in a plain module (not `.svelte.ts`) so it
// carries no reactive state and is trivially unit-testable. A move is only
// valid when the destination directory is neither one of the dragged sources
// nor a descendant of one (which would mean dropping a folder into itself).

/**
 * Whether moving `sources` into `destination` is a legal move. It is illegal
 * when there is nothing to move, when the destination IS one of the sources,
 * or when the destination sits inside one of the sources (a folder cannot be
 * moved into its own subtree). Only a true path boundary counts, so "/a" is
 * not treated as an ancestor of the sibling "/ab".
 */
export function isValidDrop(sources: string[], destination: string): boolean {
    if (sources.length === 0) {
        return false;
    }
    for (const source of sources) {
        if (destination === source || destination.startsWith(`${source}/`)) {
            return false;
        }
    }
    return true;
}
