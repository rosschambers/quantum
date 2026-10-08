// A tiny memoization layer over `buildRows(..., ALWAYS_EXPANDED)`, shared by
// every caller in `DiffView.svelte` that walks a file's FULL row list
// (search and the change-marks ruler) independent of whatever regions are
// actually collapsed in the rendered view. Without this, every keystroke in
// the search box rebuilt every entry's full row list from scratch even
// though the query never affects that list — only which rows match it.
// Keyed by the pair of git blob ids plus the language (two files with the
// same content diff differently highlighted/scoped by language), mirroring
// `lineDiffCache.ts`'s cache shape. When either side has no blob (an
// untracked file, a deletion, or a pair-mode comparison with no git
// identity) the memo is bypassed entirely and `buildRows` runs directly:
// unconditionally correct, just not cached for that case.
import { buildRows, type DiffRow } from './rows';
import type { DiffItem } from './lineDiff';

/** Bounds memory: the oldest entry is evicted once the cache is full. */
const MAX_ENTRIES = 64;

const cache = new Map<string, DiffRow[]>();

/**
 * A fake "expanded" set whose `has()` always returns true, so `buildRows`
 * returns every line as its own `context`/`removed`/`added` row — no
 * `collapsed` rows at all — purely as a way to walk a file's full row list
 * without caring about any file's actual current collapse state.
 */
class AlwaysExpandedKeys extends Set<string> {
	override has(): boolean {
		return true;
	}
}
const ALWAYS_EXPANDED: ReadonlySet<string> = new AlwaysExpandedKeys();

/**
 * Same contract as `buildRows(items, oldLines, newLines, language,
 * ALWAYS_EXPANDED)`, but returns the cached result for a repeated
 * `(oldBlob, newBlob, language)` triple instead of recomputing it.
 */
export function cachedExpandedRows(
	oldBlob: string | undefined,
	newBlob: string | undefined,
	language: string | undefined,
	items: readonly DiffItem[],
	oldLines: readonly string[],
	newLines: readonly string[],
): DiffRow[] {
	if (oldBlob === undefined || newBlob === undefined) {
		return buildRows(items, oldLines, newLines, language, ALWAYS_EXPANDED);
	}

	const key = `${oldBlob}:${newBlob}:${language ?? ''}`;
	const cached = cache.get(key);
	if (cached) {
		return cached;
	}

	const result = buildRows(items, oldLines, newLines, language, ALWAYS_EXPANDED);
	if (cache.size >= MAX_ENTRIES) {
		const oldestKey = cache.keys().next().value;
		if (oldestKey !== undefined) {
			cache.delete(oldestKey);
		}
	}
	cache.set(key, result);
	return result;
}

/** Test-only escape hatch: clears every memoized entry. */
export function clearRowsCache(): void {
	cache.clear();
}
