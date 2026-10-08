import { describe, test, expect, vi, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import DiffRows from './DiffRows.svelte';
import type { DiffRow } from './rows';
import type { DiffToken } from './highlightLines';
import type { SearchRanges } from './DiffRows.svelte';

afterEach(() => {
	cleanup();
	document.body.innerHTML = '';
	window.getSelection()?.removeAllRanges();
});

function tokenLine(text: string): DiffToken[] {
	return text.length === 0 ? [] : [{ text, classes: '' }];
}

function tokensFor(lines: string[]): DiffToken[][] {
	return lines.map(tokenLine);
}

describe('DiffRows (unified)', () => {
	test('renders old and new line numbers, removed/added markers, and tint classes', () => {
		const oldLines = ['unchanged', 'old text'];
		const newLines = ['unchanged', 'new text'];
		const rows: DiffRow[] = [
			{ kind: 'context', oldIndex: 0, newIndex: 0 },
			{ kind: 'removed', oldIndex: 1, emphasis: null },
			{ kind: 'added', newIndex: 1, emphasis: null },
		];
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'unified',
				oldTokens: tokensFor(oldLines),
				newTokens: tokensFor(newLines),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const renderedRows = container.querySelectorAll('.row');
		expect(renderedRows).toHaveLength(3);

		const contextRow = renderedRows[0];
		const lineNumbers = contextRow.querySelectorAll('.line-number');
		expect(lineNumbers[0].textContent).toBe('1');
		expect(lineNumbers[1].textContent).toBe('1');
		expect(contextRow.querySelector('.code')?.textContent).toBe('unchanged');

		const removedRow = renderedRows[1];
		expect(removedRow.classList.contains('removed')).toBe(true);
		const removedNumbers = removedRow.querySelectorAll('.line-number');
		expect(removedNumbers[0].textContent).toBe('2');
		expect(removedNumbers[1].textContent).toBe('');
		const removedCode = removedRow.querySelector('.code') as HTMLElement;
		expect(removedCode.textContent).toBe('old text');
		expect(removedCode.dataset.side).toBe('old');
		expect(removedCode.dataset.rowKind).toBe('removed');

		const addedRow = renderedRows[2];
		expect(addedRow.classList.contains('added')).toBe(true);
		const addedNumbers = addedRow.querySelectorAll('.line-number');
		expect(addedNumbers[0].textContent).toBe('');
		expect(addedNumbers[1].textContent).toBe('2');
		const addedCode = addedRow.querySelector('.code') as HTMLElement;
		expect(addedCode.textContent).toBe('new text');
		expect(addedCode.dataset.side).toBe('new');
		expect(addedCode.dataset.rowKind).toBe('added');
	});

	test('clicking a collapsed row calls onExpand with its key', async () => {
		const rows: DiffRow[] = [
			{ kind: 'collapsed', key: 'collapse-0', hiddenCount: 5, firstNewIndex: 0, scopeLine: '' },
		];
		const onExpand = vi.fn();
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'unified',
				oldTokens: tokensFor(['a', 'b', 'c', 'd', 'e']),
				newTokens: tokensFor(['a', 'b', 'c', 'd', 'e']),
				onExpand,
				onRecollapse: vi.fn(),
			},
		});

		const collapsedElement = container.querySelector('.collapsed') as HTMLElement;
		expect(collapsedElement).not.toBeNull();
		expect(collapsedElement.textContent).toContain('5');
		await fireEvent.click(collapsedElement);
		expect(onExpand).toHaveBeenCalledWith('collapse-0');
	});

	test('recollapse rows call onRecollapse with the key for both the top and bottom bar', async () => {
		const rows: DiffRow[] = [
			{ kind: 'recollapse', key: 'collapse-0', count: 5, position: 'top' },
			{ kind: 'context', oldIndex: 0, newIndex: 0 },
			{ kind: 'recollapse', key: 'collapse-0', count: 5, position: 'bottom' },
		];
		const onRecollapse = vi.fn();
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'unified',
				oldTokens: tokensFor(['a']),
				newTokens: tokensFor(['a']),
				onExpand: vi.fn(),
				onRecollapse,
			},
		});

		const recollapseElements = container.querySelectorAll('.recollapse');
		expect(recollapseElements).toHaveLength(2);
		await fireEvent.click(recollapseElements[0]);
		await fireEvent.click(recollapseElements[1]);
		expect(onRecollapse).toHaveBeenCalledTimes(2);
		expect(onRecollapse).toHaveBeenCalledWith('collapse-0');
	});

	test('dispatching a copy event over a programmatic selection yields the clean clipboard text', async () => {
		const rows: DiffRow[] = [
			{ kind: 'context', oldIndex: 0, newIndex: 0 },
			{ kind: 'removed', oldIndex: 1, emphasis: null },
			{ kind: 'added', newIndex: 1, emphasis: null },
		];
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'unified',
				oldTokens: tokensFor(['context line', 'old text']),
				newTokens: tokensFor(['context line', 'new text']),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const root = container.querySelector('.diff-root') as HTMLElement;
		const codeCells = root.querySelectorAll('.code');
		expect(codeCells).toHaveLength(3);

		const range = document.createRange();
		range.setStart(codeCells[0].firstChild ?? codeCells[0], 0);
		const lastCell = codeCells[2];
		const lastNode = lastCell.firstChild ?? lastCell;
		range.setEnd(lastNode, lastNode.textContent?.length ?? 0);
		const selection = window.getSelection()!;
		selection.removeAllRanges();
		selection.addRange(range);

		const event = new Event('copy', { bubbles: true, cancelable: true }) as unknown as ClipboardEvent;
		const setData = vi.fn();
		Object.defineProperty(event, 'clipboardData', { value: { setData }, configurable: true });
		const preventDefaultSpy = vi.spyOn(event, 'preventDefault');

		root.dispatchEvent(event);

		// Unified view drops the removed cell, so only the context and added lines survive.
		expect(setData).toHaveBeenCalledWith('text/plain', 'context line\nnew text');
		expect(preventDefaultSpy).toHaveBeenCalled();
	});

	test('pressing Enter or Space on a collapsed row calls onExpand, same as a click', async () => {
		const rows: DiffRow[] = [
			{ kind: 'collapsed', key: 'collapse-0', hiddenCount: 5, firstNewIndex: 0, scopeLine: '' },
		];
		const onExpand = vi.fn();
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'unified',
				oldTokens: tokensFor(['a', 'b', 'c', 'd', 'e']),
				newTokens: tokensFor(['a', 'b', 'c', 'd', 'e']),
				onExpand,
				onRecollapse: vi.fn(),
			},
		});

		const collapsedElement = container.querySelector('.collapsed') as HTMLElement;
		await fireEvent.keyDown(collapsedElement, { key: 'Enter' });
		expect(onExpand).toHaveBeenCalledWith('collapse-0');
		onExpand.mockClear();
		await fireEvent.keyDown(collapsedElement, { key: ' ' });
		expect(onExpand).toHaveBeenCalledWith('collapse-0');
	});

	test('pressing Enter or Space on a recollapse row calls onRecollapse, same as a click', async () => {
		const rows: DiffRow[] = [{ kind: 'recollapse', key: 'collapse-0', count: 5, position: 'top' }];
		const onRecollapse = vi.fn();
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'unified',
				oldTokens: tokensFor(['a']),
				newTokens: tokensFor(['a']),
				onExpand: vi.fn(),
				onRecollapse,
			},
		});

		const recollapseElement = container.querySelector('.recollapse') as HTMLElement;
		await fireEvent.keyDown(recollapseElement, { key: 'Enter' });
		expect(onRecollapse).toHaveBeenCalledWith('collapse-0');
		onRecollapse.mockClear();
		await fireEvent.keyDown(recollapseElement, { key: ' ' });
		expect(onRecollapse).toHaveBeenCalledWith('collapse-0');
	});

	test('tags unified rows with data-old-index / data-new-index for the overview ruler to anchor on', () => {
		const rows: DiffRow[] = [
			{ kind: 'context', oldIndex: 0, newIndex: 0 },
			{ kind: 'removed', oldIndex: 1, emphasis: null },
			{ kind: 'added', newIndex: 1, emphasis: null },
		];
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'unified',
				oldTokens: tokensFor(['unchanged', 'old text']),
				newTokens: tokensFor(['unchanged', 'new text']),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const renderedRows = container.querySelectorAll('.row');
		expect((renderedRows[0] as HTMLElement).dataset.oldIndex).toBe('0');
		expect((renderedRows[0] as HTMLElement).dataset.newIndex).toBe('0');
		// Removed rows have no new-side line at all.
		expect((renderedRows[1] as HTMLElement).dataset.oldIndex).toBe('1');
		expect((renderedRows[1] as HTMLElement).dataset.newIndex).toBeUndefined();
		// Added rows have no old-side line at all.
		expect((renderedRows[2] as HTMLElement).dataset.oldIndex).toBeUndefined();
		expect((renderedRows[2] as HTMLElement).dataset.newIndex).toBe('1');
	});

	test('tags a collapsed row with data-first-new-index and data-hidden-count, alongside data-hidden-lines', () => {
		const rows: DiffRow[] = [{ kind: 'collapsed', key: 'collapse-0', hiddenCount: 5, firstNewIndex: 3, scopeLine: '' }];
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'unified',
				oldTokens: tokensFor(['a', 'b', 'c', 'd', 'e']),
				newTokens: tokensFor(['a', 'b', 'c', 'd', 'e']),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});
		const collapsedElement = container.querySelector('.collapsed') as HTMLElement;
		expect(collapsedElement.dataset.firstNewIndex).toBe('3');
		expect(collapsedElement.dataset.hiddenCount).toBe('5');
		expect(collapsedElement.dataset.hiddenLines).toBeDefined();
	});

	test('renders search-match and search-match-current marks from searchRanges', () => {
		const rows: DiffRow[] = [{ kind: 'context', oldIndex: 0, newIndex: 0 }];
		const searchRanges: SearchRanges = {
			old: new Map(),
			new: new Map([[0, { ranges: [[0, 6], [7, 11]], currentRange: [7, 11] }]]),
		};
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'unified',
				oldTokens: tokensFor(['needle here']),
				newTokens: tokensFor(['needle here']),
				searchRanges,
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const matches = container.querySelectorAll('.search-match');
		const current = container.querySelectorAll('.search-match-current');
		expect(matches.length).toBeGreaterThan(0);
		expect(current).toHaveLength(1);
		expect(current[0].textContent).toBe('here');
	});
});

describe('DiffRows (split)', () => {
	test('fills the shorter side with an empty half for an uneven removed/added block', () => {
		const rows: DiffRow[] = [
			{ kind: 'removed', oldIndex: 0, emphasis: null },
			{ kind: 'removed', oldIndex: 1, emphasis: null },
			{ kind: 'added', newIndex: 0, emphasis: null },
		];
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'split',
				oldTokens: tokensFor(['removed one', 'removed two']),
				newTokens: tokensFor(['added one']),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const emptyHalves = container.querySelectorAll('.half.empty');
		expect(emptyHalves).toHaveLength(1);
		const codeCells = container.querySelectorAll('.code');
		expect(codeCells).toHaveLength(3);
	});

	test('scrolling one side column mirrors scrollLeft onto the other side', async () => {
		const rows: DiffRow[] = [
			{ kind: 'context', oldIndex: 0, newIndex: 0 },
			{ kind: 'context', oldIndex: 1, newIndex: 1 },
		];
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'split',
				oldTokens: tokensFor(['one', 'two']),
				newTokens: tokensFor(['one', 'two']),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const oldColumn = container.querySelector('.column.old') as HTMLElement;
		const newColumn = container.querySelector('.column.new') as HTMLElement;
		expect(oldColumn).not.toBeNull();
		expect(newColumn).not.toBeNull();

		oldColumn.scrollLeft = 42;
		await fireEvent.scroll(oldColumn);
		expect(newColumn.scrollLeft).toBe(42);
	});

	function twoHunksSeparatedByCollapse(): { rows: DiffRow[]; oldLines: string[]; newLines: string[] } {
		const sameLines = Array.from({ length: 10 }, (_, index) => `same ${index}`);
		const rows: DiffRow[] = [
			{ kind: 'removed', oldIndex: 0, emphasis: null },
			{ kind: 'added', newIndex: 0, emphasis: null },
			{ kind: 'collapsed', key: 'collapse-0', hiddenCount: 5, firstNewIndex: 1, scopeLine: '' },
			{ kind: 'removed', oldIndex: 11, emphasis: null },
			{ kind: 'added', newIndex: 11, emphasis: null },
		];
		return {
			rows,
			oldLines: ['removed one', ...sameLines, 'removed two'],
			newLines: ['added one', ...sameLines, 'added two'],
		};
	}

	test('a whole file is ONE pair of columns: a collapsed region between two hunks does not start a new column pair', () => {
		const { rows, oldLines, newLines } = twoHunksSeparatedByCollapse();
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'split',
				oldTokens: tokensFor(oldLines),
				newTokens: tokensFor(newLines),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const oldColumns = container.querySelectorAll('.column.old');
		const newColumns = container.querySelectorAll('.column.new');
		expect(oldColumns).toHaveLength(1);
		expect(newColumns).toHaveLength(1);

		// Both hunks' removed-side code cells live in the SAME old column element.
		const codeCellsInOldColumn = oldColumns[0].querySelectorAll('.code[data-side="old"]');
		expect(codeCellsInOldColumn).toHaveLength(2);
		expect(codeCellsInOldColumn[0].textContent).toBe('removed one');
		expect(codeCellsInOldColumn[1].textContent).toBe('removed two');

		// Exactly one real `.collapsed` element so copy's data-hidden-lines works once.
		expect(container.querySelectorAll('.collapsed')).toHaveLength(1);
	});

	test('scrolling the old column mirrors to the new column across the whole file, past a collapsed region between hunks', async () => {
		const { rows, oldLines, newLines } = twoHunksSeparatedByCollapse();
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'split',
				oldTokens: tokensFor(oldLines),
				newTokens: tokensFor(newLines),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const oldColumn = container.querySelector('.column.old') as HTMLElement;
		const newColumn = container.querySelector('.column.new') as HTMLElement;
		oldColumn.scrollLeft = 77;
		await fireEvent.scroll(oldColumn);
		expect(newColumn.scrollLeft).toBe(77);
	});

	test('clicking the collapsed region still calls onExpand when it sits inside the single-column layout', async () => {
		const { rows, oldLines, newLines } = twoHunksSeparatedByCollapse();
		const onExpand = vi.fn();
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'split',
				oldTokens: tokensFor(oldLines),
				newTokens: tokensFor(newLines),
				onExpand,
				onRecollapse: vi.fn(),
			},
		});

		const collapsedElement = container.querySelector('.collapsed') as HTMLElement;
		await fireEvent.click(collapsedElement);
		expect(onExpand).toHaveBeenCalledWith('collapse-0');
	});

	test('copying a side-locked split selection spanning a collapsed region still returns only that side', async () => {
		const { rows, oldLines, newLines } = twoHunksSeparatedByCollapse();
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'split',
				oldTokens: tokensFor(oldLines),
				newTokens: tokensFor(newLines),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const oldColumn = container.querySelector('.column.old') as HTMLElement;
		await fireEvent.pointerDown(oldColumn);

		const root = container.querySelector('.diff-root') as HTMLElement;
		const oldCodeCells = oldColumn.querySelectorAll('.code');
		expect(oldCodeCells).toHaveLength(2);

		const range = document.createRange();
		range.setStart(oldCodeCells[0].firstChild ?? oldCodeCells[0], 0);
		const lastNode = oldCodeCells[1].firstChild ?? oldCodeCells[1];
		range.setEnd(lastNode, lastNode.textContent?.length ?? 0);
		const selection = window.getSelection()!;
		selection.removeAllRanges();
		selection.addRange(range);

		const event = new Event('copy', { bubbles: true, cancelable: true }) as unknown as ClipboardEvent;
		const setData = vi.fn();
		Object.defineProperty(event, 'clipboardData', { value: { setData }, configurable: true });
		root.dispatchEvent(event);

		// Locked to the old side: the new side's cells never survive, but the
		// collapsed region in between stays, so the clipboard is a
		// contiguous piece of the real file (the design's copy contract).
		expect(setData).toHaveBeenCalledWith(
			'text/plain',
			'removed one\nsame 0\nsame 1\nsame 2\nsame 3\nsame 4\nremoved two',
		);
	});

	test('tags split halves with data-old-index / data-new-index, and the collapsed region with data-first-new-index / data-hidden-count', () => {
		const { rows, oldLines, newLines } = twoHunksSeparatedByCollapse();
		const { container } = render(DiffRows, {
			props: {
				rows,
				layout: 'split',
				oldTokens: tokensFor(oldLines),
				newTokens: tokensFor(newLines),
				onExpand: vi.fn(),
				onRecollapse: vi.fn(),
			},
		});

		const oldColumn = container.querySelector('.column.old') as HTMLElement;
		const newColumn = container.querySelector('.column.new') as HTMLElement;
		const oldHalves = oldColumn.querySelectorAll('.half');
		const newHalves = newColumn.querySelectorAll('.half');
		expect((oldHalves[0] as HTMLElement).dataset.oldIndex).toBe('0');
		expect((newHalves[0] as HTMLElement).dataset.newIndex).toBe('0');
		expect((oldHalves[1] as HTMLElement).dataset.oldIndex).toBe('11');
		expect((newHalves[1] as HTMLElement).dataset.newIndex).toBe('11');

		const collapsedElement = container.querySelector('.collapsed') as HTMLElement;
		expect(collapsedElement.dataset.firstNewIndex).toBe('1');
		expect(collapsedElement.dataset.hiddenCount).toBe('5');
	});
});
