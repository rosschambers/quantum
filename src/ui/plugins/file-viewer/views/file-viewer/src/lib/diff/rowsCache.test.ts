import { describe, test, expect, beforeEach } from 'vitest';
import { cachedExpandedRows, clearRowsCache } from './rowsCache';
import type { DiffItem } from './lineDiff';

const oneLineChange: DiffItem[] = [{ type: 'change', removed: [0], added: [0] }];

beforeEach(() => {
	clearRowsCache();
});

describe('cachedExpandedRows', () => {
	test('returns the identical row array for a repeated blob/language triple, never recomputing', () => {
		const first = cachedExpandedRows('old-blob', 'new-blob', 'rust', oneLineChange, ['old'], ['new']);
		const second = cachedExpandedRows('old-blob', 'new-blob', 'rust', oneLineChange, ['old'], ['new']);
		expect(second).toBe(first);
	});

	test('bypasses the cache entirely when either blob is undefined', () => {
		const first = cachedExpandedRows(undefined, 'new-blob', 'rust', oneLineChange, ['old'], ['new']);
		const second = cachedExpandedRows(undefined, 'new-blob', 'rust', oneLineChange, ['old'], ['new']);
		expect(second).not.toBe(first);
		expect(second).toEqual(first);
	});

	test('different blob pairs never share a cached result', () => {
		const first = cachedExpandedRows('old-1', 'new-1', 'rust', oneLineChange, ['old'], ['new']);
		const second = cachedExpandedRows('old-2', 'new-2', 'rust', oneLineChange, ['old'], ['new']);
		expect(second).not.toBe(first);
	});

	test('the same blobs with a different language are treated as a different cache entry', () => {
		const first = cachedExpandedRows('old-blob', 'new-blob', 'rust', oneLineChange, ['old'], ['new']);
		const second = cachedExpandedRows('old-blob', 'new-blob', 'typescript', oneLineChange, ['old'], ['new']);
		expect(second).not.toBe(first);
	});

	test('the result always has every line fully expanded (no collapsed rows), matching buildRows with an always-true expandedKeys set', () => {
		const sameLines = Array.from({ length: 10 }, (_, index) => `same ${index}`);
		const items: DiffItem[] = [
			{ type: 'change', removed: [0], added: [0] },
			{ type: 'equal', oldIndex: 1, newIndex: 1 },
			...sameLines.slice(1).map((_, index) => ({ type: 'equal' as const, oldIndex: index + 2, newIndex: index + 2 })),
		];
		const rows = cachedExpandedRows('old-blob', 'new-blob', undefined, items, ['removed', ...sameLines], ['added', ...sameLines]);
		expect(rows.some((row) => row.kind === 'collapsed')).toBe(false);
	});

	test('evicts the oldest entry once past capacity', () => {
		const results: unknown[] = [];
		for (let index = 0; index < 70; index++) {
			results.push(cachedExpandedRows(`old-${index}`, `new-${index}`, 'rust', oneLineChange, ['old'], ['new']));
		}
		const recomputed = cachedExpandedRows('old-0', 'new-0', 'rust', oneLineChange, ['old'], ['new']);
		expect(recomputed).not.toBe(results[0]);
		const stillCached = cachedExpandedRows('old-69', 'new-69', 'rust', oneLineChange, ['old'], ['new']);
		expect(stillCached).toBe(results[69]);
	});
});
