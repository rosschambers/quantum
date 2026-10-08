// A tiny memoization layer over `highlightLines`, shared by every caller that
// needs the same file's whole-file syntax tokens: `DiffFile` (old and new
// side token arrays) and `ChangesSidebar` once it needs them too. Without
// this, the same highlight.js tokenization ran from scratch on every
// re-render (for example after an unrelated file's stage/unstage), for every
// file in the review. Keyed by `${blob}:${language}` — the blob alone is not
// enough because the SAME content could in principle be requested with a
// different declared language (a rename across extensions). When there is no
// blob (an untracked file, a deletion, or a pair-mode comparison with no git
// identity at all) the memo is bypassed entirely and `highlightLines` runs
// directly: unconditionally correct, just not cached for that case.
import { highlightLines, type DiffToken } from './highlightLines';

/** Bounds memory: the oldest entry is evicted once the cache is full. */
const MAX_ENTRIES = 64;

const cache = new Map<string, DiffToken[][]>();

/**
 * Same contract as `highlightLines(content, language)`, but returns the
 * cached result for a repeated `(blob, language)` pair instead of
 * recomputing it.
 */
export function cachedHighlightLines(
	content: string,
	language: string | undefined,
	blob: string | undefined,
): DiffToken[][] {
	if (blob === undefined) {
		return highlightLines(content, language);
	}

	const key = `${blob}:${language ?? ''}`;
	const cached = cache.get(key);
	if (cached) {
		return cached;
	}

	const result = highlightLines(content, language);
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
export function clearHighlightCache(): void {
	cache.clear();
}
