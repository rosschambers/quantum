// The file-explorer's in-view clipboard, written as a Svelte 5 runes module so
// its single piece of state is reactive: menu items that depend on whether the
// clipboard holds anything (a Paste that should be disabled when empty) read it
// through `getClipboard` inside a tracked scope and update automatically. The
// clipboard is intentionally view-local — Cut and Copy record paths here and
// Paste turns them into a `move` or `copy` `FileOperation` against the target
// directory; nothing touches the system clipboard (that is `Copy path`'s job).

/** Whether a clipboard entry should move (cut) or copy (copy) on paste. */
export type ClipboardOperation = 'copy' | 'cut';

/** The clipboard payload: the pending operation and the paths it applies to. */
export interface ClipboardContents {
    operation: ClipboardOperation;
    paths: string[];
}

let contents = $state<ClipboardContents | null>(null);

/** Record a cut or copy of the given paths, replacing any earlier contents. */
export function setClipboard(operation: ClipboardOperation, paths: string[]): void {
    contents = { operation, paths };
}

/** The current clipboard contents, or null when the clipboard is empty. */
export function getClipboard(): ClipboardContents | null {
    return contents;
}

/** Empty the clipboard. */
export function clearClipboard(): void {
    contents = null;
}
