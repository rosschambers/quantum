// Clean clipboard text for diff mode: only file text is ever copied, never
// line numbers, signs, region labels, or split-view filler — ported from
// the qv diff playground's `selectedCodeText` (docs/playgrounds/
// qv-diff-playground.html). Split into two pieces: `collectSelectedCells`
// is the DOM adapter that reads a `Selection` into an ordered, already-
// trimmed list of cells; `buildClipboardText` is the pure function that
// turns that list into the final clipboard string.
//
// DOM contract `collectSelectedCells` relies on (owned by the future
// `DiffRows.svelte` component, documented here so that component matches
// it): a code cell is any element with class `code`, carrying
// `data-side="old"` or `data-side="new"` and `data-row-kind="context"`,
// `"removed"`, or `"added"`, whose `textContent` is exactly the line's
// code text (no gutter, no sign, nothing else). A collapsed region is any
// element with class `collapsed`, carrying `data-hidden-lines` as a JSON
// array of the hidden lines' exact text (not rendered, so the clipboard
// can reconstruct a contiguous piece of the real file without needing to
// expand the region first).
export type SelectedCell =
	| { kind: 'code'; text: string; side: 'old' | 'new'; rowKind: 'context' | 'removed' | 'added' }
	| { kind: 'hidden'; lines: string[] };

export interface ClipboardOptions {
	layout: 'unified' | 'split';
	/** Which side a split-view selection started on; `null` when layout is 'unified' or nothing has locked a side yet. */
	lockedSide: 'old' | 'new' | null;
}

/**
 * Rebuilds clipboard text from an ordered list of selected cells. Order of
 * operations matters and mirrors the playground exactly: whether a hidden
 * (collapsed-region) cell counts is decided FIRST, using the first/last
 * KEPT-CODE-CELL bounds of the full, unfiltered list — only then is the
 * split-side or unified-removed filter applied. Deciding the hidden bounds
 * after already filtering by side/kind would wrongly drop a hidden region
 * that sits between two same-side cells (its neighbors on the OTHER side
 * would otherwise still count as the "first/last code cell").
 */
export function buildClipboardText(cells: readonly SelectedCell[], options: ClipboardOptions): string {
	const firstCodeIndex = cells.findIndex((cell) => cell.kind === 'code');
	let lastCodeIndex = -1;
	for (let index = cells.length - 1; index >= 0; index--) {
		if (cells[index].kind === 'code') {
			lastCodeIndex = index;
			break;
		}
	}

	let kept = cells.filter((cell, index) => {
		if (cell.kind === 'code') {
			return true;
		}
		// A collapsed region only survives when it sits strictly BETWEEN the
		// first and last kept code cell, so the clipboard is always a
		// contiguous piece of the real file, never a leading/trailing label.
		return firstCodeIndex !== -1 && index > firstCodeIndex && index < lastCodeIndex;
	});

	if (options.layout === 'split' && options.lockedSide) {
		const lockedSide = options.lockedSide;
		kept = kept.filter((cell) => cell.kind === 'hidden' || cell.side === lockedSide);
	} else if (options.layout === 'unified') {
		kept = kept.filter((cell) => cell.kind === 'hidden' || cell.rowKind !== 'removed');
	}

	return kept.map((cell) => (cell.kind === 'hidden' ? cell.lines.join('\n') : cell.text)).join('\n');
}

function textOffsetWithin(element: Element, container: Node, offset: number): number {
	const probe = document.createRange();
	probe.selectNodeContents(element);
	probe.setEnd(container, offset);
	return probe.toString().length;
}

function trimmedCodeText(element: HTMLElement, range: Range): string {
	const text = element.textContent ?? '';
	let start = 0;
	let end = text.length;
	if (element.contains(range.startContainer)) {
		start = textOffsetWithin(element, range.startContainer, range.startOffset);
	}
	if (element.contains(range.endContainer)) {
		end = textOffsetWithin(element, range.endContainer, range.endOffset);
	}
	return text.slice(start, end);
}

/**
 * Reads the current selection into an ordered list of `SelectedCell`s,
 * bounded to the visible `.code` / `.collapsed` rows inside `root` — never
 * every text node in the document — using `Range.intersectsNode` only over
 * those elements. The first and last matched code cell are trimmed to the
 * selection's actual start/end text offset, so a partial-line selection
 * copies only the selected part. Returns `[]` for an empty/collapsed
 * selection or one that does not touch `root` at all.
 */
export function collectSelectedCells(selection: Selection, root: HTMLElement): SelectedCell[] {
	if (selection.rangeCount === 0 || selection.isCollapsed) {
		return [];
	}
	const range = selection.getRangeAt(0);
	if (!root.contains(range.commonAncestorContainer)) {
		return [];
	}

	const elements = Array.from(root.querySelectorAll<HTMLElement>('.code, .collapsed')).filter((element) =>
		range.intersectsNode(element),
	);

	return elements.map((element): SelectedCell => {
		if (element.classList.contains('collapsed')) {
			const raw = element.dataset.hiddenLines;
			let lines: string[] = [];
			if (raw) {
				try {
					const parsed = JSON.parse(raw);
					if (Array.isArray(parsed)) {
						lines = parsed;
					}
				} catch {
					lines = [];
				}
			}
			return { kind: 'hidden', lines };
		}
		const side: 'old' | 'new' = element.dataset.side === 'new' ? 'new' : 'old';
		const rowKind: 'context' | 'removed' | 'added' =
			element.dataset.rowKind === 'removed' ? 'removed' : element.dataset.rowKind === 'added' ? 'added' : 'context';
		return { kind: 'code', text: trimmedCodeText(element, range), side, rowKind };
	});
}
