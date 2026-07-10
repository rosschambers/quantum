// The file-explorer's current drag payload, written as a Svelte 5 runes module
// so its single piece of state is shared across every pane, pin, and tree node
// in the one WebView document. A drag begins on one row and ends on a target
// somewhere else entirely; a shared module carries the dragged paths between
// them without depending on `dataTransfer` round-trips, which jsdom does not
// implement in tests. The `dragstart` handler still writes the paths to
// `dataTransfer` for completeness, but the drop handlers read them from here.

/** The active drag payload: the set of paths being moved, or null when idle. */
interface DragPayload {
    sources: string[];
}

let payload = $state<DragPayload | null>(null);

/** Record the start of a drag carrying the given source paths. */
export function beginDrag(sources: string[]): void {
    payload = { sources };
}

/** Clear the drag payload once the drag ends or a drop completes. */
export function endDrag(): void {
    payload = null;
}

/** The paths of the active drag, or null when no drag is in progress. */
export function getDragSources(): string[] | null {
    return payload === null ? null : payload.sources;
}
