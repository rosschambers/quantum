import { describe, test, expect, beforeEach } from 'vitest';
import { cachedHighlightLines, clearHighlightCache } from './highlightCache';

beforeEach(() => {
	clearHighlightCache();
});

describe('cachedHighlightLines', () => {
	test('returns the identical result array for a repeated blob and language, never recomputing', () => {
		const first = cachedHighlightLines('const a = 1;', 'typescript', 'blob-1');
		const second = cachedHighlightLines('const a = 1;', 'typescript', 'blob-1');
		expect(second).toBe(first);
	});

	test('bypasses the cache entirely when the blob is undefined', () => {
		const first = cachedHighlightLines('const a = 1;', 'typescript', undefined);
		const second = cachedHighlightLines('const a = 1;', 'typescript', undefined);
		expect(second).not.toBe(first);
		expect(second).toEqual(first);
	});

	test('different blobs never share a cached result', () => {
		const first = cachedHighlightLines('const a = 1;', 'typescript', 'blob-1');
		const second = cachedHighlightLines('const b = 2;', 'typescript', 'blob-2');
		expect(second).not.toBe(first);
	});

	test('the same blob with a different language is not shared (keyed by blob AND language)', () => {
		const first = cachedHighlightLines('const a = 1;', 'typescript', 'blob-1');
		const second = cachedHighlightLines('const a = 1;', 'javascript', 'blob-1');
		expect(second).not.toBe(first);
	});

	test('the result still matches a direct highlightLines call for the same inputs', () => {
		const cached = cachedHighlightLines('plain text', undefined, 'blob-1');
		expect(cached).toEqual([[{ text: 'plain text', classes: '' }]]);
	});

	test('evicts the oldest entry once past capacity', () => {
		const results: unknown[] = [];
		for (let index = 0; index < 70; index++) {
			results.push(cachedHighlightLines(`line ${index}`, undefined, `blob-${index}`));
		}
		// Capacity is 64; entry 0 was inserted first and is long evicted by
		// the time 70 distinct blobs have been cached, so asking again
		// recomputes a fresh (non-identical) array.
		const recomputed = cachedHighlightLines('line 0', undefined, 'blob-0');
		expect(recomputed).not.toBe(results[0]);
		// A recently inserted entry is still cached.
		const stillCached = cachedHighlightLines('line 69', undefined, 'blob-69');
		expect(stillCached).toBe(results[69]);
	});
});
