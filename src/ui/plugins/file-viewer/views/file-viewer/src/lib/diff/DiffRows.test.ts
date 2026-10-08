import { describe, test, expect, vi, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import DiffRows from './DiffRows.svelte';
import type { DiffRow } from './rows';
import type { DiffToken } from './highlightLines';

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
		const lineNumbers = contextRow.querySelectorAll('.ln');
		expect(lineNumbers[0].textContent).toBe('1');
		expect(lineNumbers[1].textContent).toBe('1');
		expect(contextRow.querySelector('.code')?.textContent).toBe('unchanged');

		const removedRow = renderedRows[1];
		expect(removedRow.classList.contains('removed')).toBe(true);
		const removedNumbers = removedRow.querySelectorAll('.ln');
		expect(removedNumbers[0].textContent).toBe('2');
		expect(removedNumbers[1].textContent).toBe('');
		const removedCode = removedRow.querySelector('.code') as HTMLElement;
		expect(removedCode.textContent).toBe('old text');
		expect(removedCode.dataset.side).toBe('old');
		expect(removedCode.dataset.rowKind).toBe('removed');

		const addedRow = renderedRows[2];
		expect(addedRow.classList.contains('added')).toBe(true);
		const addedNumbers = addedRow.querySelectorAll('.ln');
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
});
