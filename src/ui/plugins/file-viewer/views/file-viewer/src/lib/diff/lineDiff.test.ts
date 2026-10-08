import { describe, test, expect } from 'vitest';
import { lineDiff } from './lineDiff';

describe('lineDiff', () => {
	test('identical arrays produce only equal items', () => {
		const lines = ['one', 'two', 'three'];
		expect(lineDiff(lines, [...lines])).toEqual([
			{ type: 'equal', oldIndex: 0, newIndex: 0 },
			{ type: 'equal', oldIndex: 1, newIndex: 1 },
			{ type: 'equal', oldIndex: 2, newIndex: 2 },
		]);
	});

	test('pure insertion is one change item with only added indices', () => {
		const items = lineDiff(['a', 'b'], ['a', 'x', 'y', 'b']);
		expect(items).toEqual([
			{ type: 'equal', oldIndex: 0, newIndex: 0 },
			{ type: 'change', removed: [], added: [1, 2] },
			{ type: 'equal', oldIndex: 1, newIndex: 3 },
		]);
	});

	test('pure deletion is one change item with only removed indices', () => {
		const items = lineDiff(['a', 'x', 'y', 'b'], ['a', 'b']);
		expect(items).toEqual([
			{ type: 'equal', oldIndex: 0, newIndex: 0 },
			{ type: 'change', removed: [1, 2], added: [] },
			{ type: 'equal', oldIndex: 3, newIndex: 1 },
		]);
	});

	test('a replaced block yields one change item with both lists', () => {
		const items = lineDiff(['a', 'old1', 'old2', 'b'], ['a', 'new1', 'new2', 'new3', 'b']);
		expect(items).toEqual([
			{ type: 'equal', oldIndex: 0, newIndex: 0 },
			{ type: 'change', removed: [1, 2], added: [1, 2, 3] },
			{ type: 'equal', oldIndex: 3, newIndex: 4 },
		]);
	});

	test('empty arrays produce no items', () => {
		expect(lineDiff([], [])).toEqual([]);
	});
});
