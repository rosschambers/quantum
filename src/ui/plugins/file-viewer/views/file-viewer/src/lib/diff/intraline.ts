// Word-level intra-line emphasis for the file viewer's diff mode. Ported
// from the qv diff playground's hand-rolled word LCS (`intraLine`), now
// built on the `diff` package's `diffWordsWithSpace`. Returns the character
// ranges to emphasize on each side, or `null` when the two lines are too
// dissimilar for word-level emphasis to be useful (the playground's 40%
// shared-character gate).
import { diffWordsWithSpace } from 'diff';

const SIMILARITY_THRESHOLD = 0.4;

export interface IntraLineEmphasis {
	removed: [number, number][];
	added: [number, number][];
}

/**
 * Diffs `oldText` against `newText` word-by-word (`diffWordsWithSpace`
 * keeps whitespace as its own tokens, so emphasis boundaries land on word
 * edges rather than splitting a run of spaces) and returns the changed
 * character ranges on each side. Adjacent ranges (touching boundaries)
 * merge into one. Returns `null` when fewer than 40% of the characters
 * (counting both sides) are shared — at that point word-level emphasis is
 * noise, not a helpful signal.
 */
export function intraLine(oldText: string, newText: string): IntraLineEmphasis | null {
	const parts = diffWordsWithSpace(oldText, newText);
	const removed: [number, number][] = [];
	const added: [number, number][] = [];
	let oldPosition = 0;
	let newPosition = 0;
	let sharedCharacters = 0;

	function pushRange(list: [number, number][], start: number, end: number): void {
		const last = list[list.length - 1];
		if (last && last[1] === start) {
			last[1] = end;
		} else {
			list.push([start, end]);
		}
	}

	for (const part of parts) {
		const length = part.value.length;
		if (!part.added && !part.removed) {
			sharedCharacters += length;
			oldPosition += length;
			newPosition += length;
		} else if (part.removed) {
			pushRange(removed, oldPosition, oldPosition + length);
			oldPosition += length;
		} else {
			pushRange(added, newPosition, newPosition + length);
			newPosition += length;
		}
	}

	const similarity = (2 * sharedCharacters) / Math.max(1, oldText.length + newText.length);
	if (similarity < SIMILARITY_THRESHOLD) {
		return null;
	}
	return { removed, added };
}
