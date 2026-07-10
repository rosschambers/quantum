// Pure predicate for the automatic preview pane. Kept in a plain module (not a
// `.svelte.ts`) so it carries no reactive state and is trivially unit-testable.
// Deterministic given its input.

import type { FileEntry } from '@quantum/client';

/**
 * Whether an entry warrants an automatic preview. True for non-directory
 * entries whose content classifies as an image, a document, or code — the set
 * the preview pane can render a thumbnail or a text snippet for. Everything
 * else (directories, archives, music, and unclassified files) is not
 * previewable, and the App leaves the 250px preview column unallocated.
 */
export function isPreviewable(entry: FileEntry): boolean {
    if (entry.kind === 'directory') {
        return false;
    }
    return (
        entry.content_kind === 'image' ||
        entry.content_kind === 'document' ||
        entry.content_kind === 'code'
    );
}
