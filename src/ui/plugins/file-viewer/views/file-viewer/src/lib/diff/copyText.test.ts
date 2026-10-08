import { describe, test, expect, afterEach } from 'vitest';
import { buildClipboardText, collectSelectedCells, type SelectedCell } from './copyText';

function codeCell(text: string, side: 'old' | 'new', rowKind: 'context' | 'removed' | 'added' = 'context'): HTMLDivElement {
	const element = document.createElement('div');
	element.className = 'code';
	element.dataset.side = side;
	element.dataset.rowKind = rowKind;
	element.textContent = text;
	return element;
}

function gutterCell(text: string): HTMLSpanElement {
	const element = document.createElement('span');
	element.className = 'ln';
	element.textContent = text;
	return element;
}

function collapsedCell(lines: string[]): HTMLDivElement {
	const element = document.createElement('div');
	element.className = 'collapsed';
	element.dataset.hiddenLines = JSON.stringify(lines);
	element.textContent = `\u2195 ${lines.length} unchanged lines`;
	return element;
}

function selectAcross(root: HTMLElement, startElement: HTMLElement, endElement: HTMLElement): Selection {
	const range = document.createRange();
	range.setStart(startElement.firstChild ?? startElement, 0);
	const endNode = endElement.firstChild ?? endElement;
	const endOffset = endNode.textContent?.length ?? 0;
	range.setEnd(endNode, endOffset);
	const selection = window.getSelection()!;
	selection.removeAllRanges();
	selection.addRange(range);
	void root;
	return selection;
}

afterEach(() => {
	document.body.innerHTML = '';
	window.getSelection()?.removeAllRanges();
});

describe('collectSelectedCells + buildClipboardText (DOM-driven, mirrors the playground)', () => {
	test('contiguous copy across a collapsed region copies the new side only', () => {
		const root = document.createElement('div');
		document.body.appendChild(root);

		const beforeOld = codeCell('old context before', 'old');
		const beforeNew = codeCell('new context before', 'new');
		const hidden = collapsedCell(Array.from({ length: 35 }, (_, index) => `hidden line ${index}`));
		const afterNew = codeCell('new context after', 'new');
		const afterOld = codeCell('old context after', 'old');
		root.append(beforeOld, beforeNew, hidden, afterNew, afterOld);

		const selection = selectAcross(root, beforeNew, afterNew);
		const cells = collectSelectedCells(selection, root);

		expect(cells).toEqual<SelectedCell[]>([
			{ kind: 'code', text: 'new context before', side: 'new', rowKind: 'context' },
			{ kind: 'hidden', lines: Array.from({ length: 35 }, (_, index) => `hidden line ${index}`) },
			{ kind: 'code', text: 'new context after', side: 'new', rowKind: 'context' },
		]);

		const text = buildClipboardText(cells, { layout: 'split', lockedSide: 'new' });
		const expectedLines = [
			'new context before',
			...Array.from({ length: 35 }, (_, index) => `hidden line ${index}`),
			'new context after',
		];
		expect(text).toBe(expectedLines.join('\n'));
	});

	test('split view: a selection locked to the old side never includes new-side text', () => {
		const root = document.createElement('div');
		document.body.appendChild(root);
		const oldCell = codeCell('old side text', 'old');
		const newCell = codeCell('new side text', 'new');
		root.append(oldCell, newCell);

		const selection = selectAcross(root, oldCell, newCell);
		const cells = collectSelectedCells(selection, root);

		expect(buildClipboardText(cells, { layout: 'split', lockedSide: 'old' })).toBe('old side text');
		expect(buildClipboardText(cells, { layout: 'split', lockedSide: 'new' })).toBe('new side text');
	});

	test('a removed line inside a unified selection is skipped', () => {
		const root = document.createElement('div');
		document.body.appendChild(root);
		const context = codeCell('context line', 'new', 'context');
		const removed = codeCell('removed line', 'old', 'removed');
		const added = codeCell('added line', 'new', 'added');
		root.append(context, removed, added);

		const selection = selectAcross(root, context, added);
		const cells = collectSelectedCells(selection, root);

		expect(buildClipboardText(cells, { layout: 'unified', lockedSide: null })).toBe('context line\nadded line');
	});

	test('gutter text never appears, even when the selection visually spans it', () => {
		const root = document.createElement('div');
		document.body.appendChild(root);
		const gutter = gutterCell('42');
		const code = codeCell('the actual line text', 'new');
		root.append(gutter, code);

		// Select from inside the gutter through the code cell.
		const selection = selectAcross(root, gutter, code);
		const cells = collectSelectedCells(selection, root);

		// The gutter is not a `.code` or `.collapsed` element, so it is never
		// collected as a cell at all.
		expect(cells).toEqual<SelectedCell[]>([{ kind: 'code', text: 'the actual line text', side: 'new', rowKind: 'context' }]);
		expect(buildClipboardText(cells, { layout: 'unified', lockedSide: null })).toBe('the actual line text');
	});

	test('an empty code cell keeps its blank line', () => {
		const cells: SelectedCell[] = [
			{ kind: 'code', text: 'before', side: 'new', rowKind: 'context' },
			{ kind: 'code', text: '', side: 'new', rowKind: 'context' },
			{ kind: 'code', text: 'after', side: 'new', rowKind: 'context' },
		];
		expect(buildClipboardText(cells, { layout: 'unified', lockedSide: null })).toBe('before\n\nafter');
	});

	test('a leading or trailing hidden cell (outside the kept code bounds) is dropped', () => {
		const cells: SelectedCell[] = [
			{ kind: 'hidden', lines: ['should not appear'] },
			{ kind: 'code', text: 'only line', side: 'new', rowKind: 'context' },
		];
		expect(buildClipboardText(cells, { layout: 'unified', lockedSide: null })).toBe('only line');
	});

	test('an empty selection returns no cells', () => {
		const root = document.createElement('div');
		document.body.appendChild(root);
		const selection = window.getSelection()!;
		selection.removeAllRanges();
		expect(collectSelectedCells(selection, root)).toEqual([]);
	});
});
