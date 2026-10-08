import { describe, test, expect, beforeEach } from 'vitest';
import { cachedLineDiff, clearLineDiffCache } from './lineDiffCache';

beforeEach(() => {
	clearLineDiffCache();
});

describe('cachedLineDiff', () => {
	test('returns the identical result array for a repeated blob pair, never recomputing', () => {
		const first = cachedLineDiff('b1', 'b2', ['a'], ['b']);
		const second = cachedLineDiff('b1', 'b2', ['a'], ['b']);
		expect(second).toBe(first);
	});

	test('bypasses the cache entirely when either blob is undefined', () => {
		const first = cachedLineDiff(undefined, 'b2', ['a'], ['b']);
		const second = cachedLineDiff(undefined, 'b2', ['a'], ['b']);
		expect(second).not.toBe(first);
		expect(second).toEqual(first);
	});

	test('different blob pairs never share a cached result', () => {
		const first = cachedLineDiff('b1', 'b2', ['a'], ['b']);
		const second = cachedLineDiff('b3', 'b4', ['a'], ['b']);
		expect(second).not.toBe(first);
	});

	test('the result still matches a direct lineDiff call for the same inputs', () => {
		const cached = cachedLineDiff('b1', 'b2', ['same', 'old'], ['same', 'new']);
		expect(cached).toEqual([
			{ type: 'equal', oldIndex: 0, newIndex: 0 },
			{ type: 'change', removed: [1], added: [1] },
		]);
	});

	test('evicts the oldest entry once past capacity', () => {
		const results: unknown[] = [];
		for (let index = 0; index < 70; index++) {
			results.push(cachedLineDiff(`old-${index}`, `new-${index}`, [`${index}`], [`${index}+1`]));
		}
		// Capacity is 64; entry 0 was inserted first and is long evicted by
		// the time 70 distinct pairs have been cached, so asking again
		// recomputes a fresh (non-identical) array.
		const recomputed = cachedLineDiff('old-0', 'new-0', ['0'], ['0+1']);
		expect(recomputed).not.toBe(results[0]);
		// A recently inserted entry is still cached.
		const stillCached = cachedLineDiff('old-69', 'new-69', ['69'], ['69+1']);
		expect(stillCached).toBe(results[69]);
	});
});
