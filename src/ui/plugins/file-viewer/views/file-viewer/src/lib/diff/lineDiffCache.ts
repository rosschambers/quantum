// A tiny memoization layer over `lineDiff`, shared by every caller that
// needs the same file's line diff: `DiffFile` (to build rows), `ChangesSidebar`
// and `DiffView` (both only need the +N/-M counts). Without this, the same
// Myers diff was recomputed from scratch by each caller for the same file.
// Keyed by the pair of git blob ids — a stable identity independent of
// however many separate `oldLines`/`newLines` arrays each caller happens to
// derive. When either side has no blob (an untracked file, a deletion, or a
// pair-mode comparison with no git identity at all) the memo is bypassed
// entirely and `lineDiff` runs directly: unconditionally correct, just not
// cached for that case.
import { lineDiff, type DiffItem } from './lineDiff';
import { clearHighlightCache } from './highlightCache';

/** Bounds memory: the oldest entry is evicted once the cache is full. */
const MAX_ENTRIES = 64;

const cache = new Map<string, DiffItem[]>();

/**
 * Same contract as `lineDiff(oldLines, newLines)`, but returns the cached
 * result for a repeated `(oldBlob, newBlob)` pair instead of recomputing it.
 */
export function cachedLineDiff(
	oldBlob: string | undefined,
	newBlob: string | undefined,
	oldLines: readonly string[],
	newLines: readonly string[],
): DiffItem[] {
	if (oldBlob === undefined || newBlob === undefined) {
		return lineDiff(oldLines, newLines);
	}

	const key = `${oldBlob}:${newBlob}`;
	const cached = cache.get(key);
	if (cached) {
		return cached;
	}

	const result = lineDiff(oldLines, newLines);
	if (cache.size >= MAX_ENTRIES) {
		const oldestKey = cache.keys().next().value;
		if (oldestKey !== undefined) {
			cache.delete(oldestKey);
		}
	}
	cache.set(key, result);
	return result;
}

/**
 * Test-only escape hatch: clears every memoized entry.
 *
 * Also clears the sibling `highlightCache` module's memo. Both caches are
 * keyed off the same git blob ids and exist for the same reason (avoiding a
 * recompute storm across `DiffFile`/`ChangesSidebar` re-renders), so any
 * caller clearing one for test isolation almost certainly needs the other
 * cleared too — most notably `DiffView.test.ts`, which already calls this
 * function in its `afterEach` specifically because its fixtures reuse
 * placeholder blob ids across tests with genuinely different content (see
 * the comment there). Routing both teardowns through this one entrypoint
 * keeps that existing test correctly isolated without needing its own
 * change.
 */
export function clearLineDiffCache(): void {
	cache.clear();
	clearHighlightCache();
}
