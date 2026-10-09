// Line-level diff for the file viewer's diff mode. Ported from the qv diff
// playground's hand-rolled LCS (docs/playgrounds/qv-diff-playground.html,
// `lineDiff`), now built on the `diff` package's `diffArrays` so the
// algorithm itself (Myers) comes from a maintained dependency while the
// output shape stays the one the playground proved out: a flat list of
// `equal` and `change` items, with consecutive removed/added runs merged
// into a single `change` item (never a bare `removed`-only or `added`-only
// item sitting next to the `change` it belongs with).
import { diffArrays } from 'diff';

export type DiffItem =
	| { type: 'equal'; oldIndex: number; newIndex: number }
	| { type: 'change'; removed: number[]; added: number[] };

/**
 * Diffs two arrays of lines by strict equality and returns one item per
 * equal line and one item per contiguous block of removed/added lines.
 * `diffArrays` always emits a removed run immediately followed by its
 * matching added run for a replaced block (never interleaved, never split
 * by an equal line in between), so folding consecutive removed/added
 * change parts into one `change` item is always correct here.
 */
export function lineDiff(oldLines: readonly string[], newLines: readonly string[]): DiffItem[] {
	const changes = diffArrays(oldLines as string[], newLines as string[]);
	const items: DiffItem[] = [];
	let oldIndex = 0;
	let newIndex = 0;

	function currentChange(): { type: 'change'; removed: number[]; added: number[] } {
		const last = items[items.length - 1];
		if (last && last.type === 'change') {
			return last;
		}
		const created: { type: 'change'; removed: number[]; added: number[] } = {
			type: 'change',
			removed: [],
			added: [],
		};
		items.push(created);
		return created;
	}

	for (const part of changes) {
		if (!part.added && !part.removed) {
			for (let count = 0; count < part.value.length; count++) {
				items.push({ type: 'equal', oldIndex, newIndex });
				oldIndex++;
				newIndex++;
			}
		} else if (part.removed) {
			const change = currentChange();
			for (let count = 0; count < part.value.length; count++) {
				change.removed.push(oldIndex);
				oldIndex++;
			}
		} else {
			const change = currentChange();
			for (let count = 0; count < part.value.length; count++) {
				change.added.push(newIndex);
				newIndex++;
			}
		}
	}

	return items;
}

/**
 * Split file content into lines the way git counts them: a final newline ends
 * the last line rather than starting an empty one, so "gone\n" is ONE line.
 * Absent or empty content has no lines.
 */
export function splitContentLines(content: string | undefined): string[] {
	if (!content) {
		return [];
	}
	const body = content.endsWith('\n') ? content.slice(0, -1) : content;
	return body.split('\n');
}
