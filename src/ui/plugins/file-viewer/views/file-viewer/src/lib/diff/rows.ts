// Turns a line diff into the rows a diff view renders: unchanged context,
// removed/added lines (with intra-line emphasis), and collapsed regions for
// long unchanged runs. Ported from the qv diff playground's `buildRows` /
// `findScope` (docs/playgrounds/qv-diff-playground.html) with only the
// settled options kept: exactly 3 context lines, a run collapses only when
// more than 2 lines would be hidden, and an expanded region always shows
// "Hide N expanded lines" bars at BOTH top and bottom (the playground also
// offered a single top bar or no bars; those options are gone).
import type { DiffItem } from './lineDiff';
import { intraLine } from './intraline';
import { highlightCode, escapeHtml } from '../highlighter';

/** Unchanged lines kept around each change before a run collapses. */
const CONTEXT_SIZE = 3;
/** A run collapses only when MORE than this many lines would be hidden. */
const COLLAPSE_THRESHOLD = 2;

export type EmphasisRanges = readonly (readonly [number, number])[];

export type DiffRow =
	| { kind: 'context'; oldIndex: number; newIndex: number }
	| { kind: 'removed'; oldIndex: number; emphasis: EmphasisRanges | null }
	| { kind: 'added'; newIndex: number; emphasis: EmphasisRanges | null }
	| { kind: 'collapsed'; key: string; hiddenCount: number; firstNewIndex: number; scopeLine: string }
	| { kind: 'recollapse'; key: string; count: number; position: 'top' | 'bottom' };

/**
 * One row of a side-by-side (split) layout. `full` rows (collapsed /
 * recollapse) span the whole width and carry their row in `left`, with
 * `right` unused. Otherwise `left`/`right` are the old/new halves of a
 * context row (both populated, same row) or of a change block (`null`
 * filling whichever side ran out first in an uneven removed/added pairing).
 */
export interface SplitViewRow {
	readonly left: DiffRow | null;
	readonly right: DiffRow | null;
	readonly full: boolean;
}

/** Scope patterns per language, keyed by the `language_for_extension` id. Languages with no entry never show a scope line. */
const SCOPE_PATTERNS: Readonly<Record<string, RegExp>> = {
	rust: /^\s*(pub\s+)?(fn|enum|struct|impl|mod|trait)\b/,
	typescript: /^\s*(export\s+)?(function|class|interface|const|type)\b/,
	javascript: /^\s*(export\s+)?(function|class|interface|const|type)\b/,
	toml: /^\s*\[/,
	markdown: /^#/,
};

/**
 * Finds the enclosing scope line (the nearest preceding line matching the
 * language's scope pattern, searching backward from `targetNewIndex`) and
 * returns it syntax-highlighted and trimmed. Returns `""` when the
 * language has no scope pattern or none is found — callers render that as
 * no scope label, not an error.
 */
function findScope(
	oldLines: readonly string[],
	newLines: readonly string[],
	language: string | undefined,
	targetNewIndex: number,
): string {
	if (!language) {
		return '';
	}
	const pattern = SCOPE_PATTERNS[language];
	if (!pattern) {
		return '';
	}
	const lines = newLines.length > 0 ? newLines : oldLines;
	if (lines.length === 0) {
		return '';
	}
	const start = Math.min(Math.max(targetNewIndex, 0), lines.length - 1);
	for (let index = start; index >= 0; index--) {
		if (pattern.test(lines[index])) {
			const text = lines[index].trim();
			return highlightCode(text, language) || escapeHtml(text);
		}
	}
	return '';
}

interface EqualEntry {
	oldIndex: number;
	newIndex: number;
}
interface RunGroup {
	type: 'run';
	entries: EqualEntry[];
}
interface ChangeGroup {
	type: 'change';
	removed: number[];
	added: number[];
}
type Group = RunGroup | ChangeGroup;

function groupItems(items: readonly DiffItem[]): Group[] {
	const groups: Group[] = [];
	for (const item of items) {
		const last = groups[groups.length - 1];
		if (item.type === 'equal') {
			if (last && last.type === 'run') {
				last.entries.push({ oldIndex: item.oldIndex, newIndex: item.newIndex });
			} else {
				groups.push({ type: 'run', entries: [{ oldIndex: item.oldIndex, newIndex: item.newIndex }] });
			}
		} else {
			groups.push({ type: 'change', removed: [...item.removed], added: [...item.added] });
		}
	}
	return groups;
}

/**
 * Builds the row list for one diffed file. `items` is `lineDiff`'s output
 * (computed separately so a caller that also needs addition/deletion
 * counts does not diff twice); `expandedKeys` holds the `key` of every
 * currently-expanded collapsed region (from `DiffRow.collapsed.key`), so
 * this stays a pure function of its inputs with no internal view state.
 */
export function buildRows(
	items: readonly DiffItem[],
	oldLines: readonly string[],
	newLines: readonly string[],
	language: string | undefined,
	expandedKeys: ReadonlySet<string>,
): DiffRow[] {
	const groups = groupItems(items);
	const rows: DiffRow[] = [];

	groups.forEach((group, groupIndex) => {
		const isFirst = groupIndex === 0;
		const isLast = groupIndex === groups.length - 1;

		if (group.type === 'run') {
			const length = group.entries.length;
			const keepHead = isFirst ? 0 : Math.min(CONTEXT_SIZE, length);
			const keepTail = isLast ? 0 : Math.min(CONTEXT_SIZE, length);
			const hiddenCount = length - keepHead - keepTail;
			const key = `collapse-${groupIndex}`;

			if (hiddenCount <= COLLAPSE_THRESHOLD) {
				for (const entry of group.entries) {
					rows.push({ kind: 'context', oldIndex: entry.oldIndex, newIndex: entry.newIndex });
				}
				return;
			}

			for (const entry of group.entries.slice(0, keepHead)) {
				rows.push({ kind: 'context', oldIndex: entry.oldIndex, newIndex: entry.newIndex });
			}

			const hiddenEntries = group.entries.slice(keepHead, length - keepTail);
			if (expandedKeys.has(key)) {
				rows.push({ kind: 'recollapse', key, count: hiddenCount, position: 'top' });
				for (const entry of hiddenEntries) {
					rows.push({ kind: 'context', oldIndex: entry.oldIndex, newIndex: entry.newIndex });
				}
				rows.push({ kind: 'recollapse', key, count: hiddenCount, position: 'bottom' });
			} else {
				rows.push({
					kind: 'collapsed',
					key,
					hiddenCount,
					firstNewIndex: hiddenEntries[0].newIndex,
					scopeLine: '',
				});
			}

			for (const entry of group.entries.slice(length - keepTail)) {
				rows.push({ kind: 'context', oldIndex: entry.oldIndex, newIndex: entry.newIndex });
			}
		} else {
			const pairCount = Math.min(group.removed.length, group.added.length);
			const pairs: ({ removed: EmphasisRanges; added: EmphasisRanges } | null)[] = [];
			for (let index = 0; index < pairCount; index++) {
				const oldText = oldLines[group.removed[index]];
				const newText = newLines[group.added[index]];
				pairs[index] = intraLine(oldText, newText);
			}
			group.removed.forEach((oldIndex, index) => {
				rows.push({ kind: 'removed', oldIndex, emphasis: pairs[index] ? pairs[index]!.removed : null });
			});
			group.added.forEach((newIndex, index) => {
				rows.push({ kind: 'added', newIndex, emphasis: pairs[index] ? pairs[index]!.added : null });
			});
		}
	});

	fillScopes(rows, oldLines, newLines, language);
	return rows;
}

/**
 * Fills in each `collapsed` row's `scopeLine`: the scope of the first
 * CHANGED line in the following hunk (more useful than git's "line before
 * the hunk"), found by scanning forward until the next collapsed row.
 */
function fillScopes(
	rows: DiffRow[],
	oldLines: readonly string[],
	newLines: readonly string[],
	language: string | undefined,
): void {
	rows.forEach((row, index) => {
		if (row.kind !== 'collapsed') {
			return;
		}
		let lastNewIndex = row.firstNewIndex;
		let firstChangeNew: number | null = null;
		for (let next = index + 1; next < rows.length && rows[next].kind !== 'collapsed'; next++) {
			const candidate = rows[next];
			if (candidate.kind === 'recollapse') {
				continue;
			}
			if (candidate.kind === 'context') {
				lastNewIndex = candidate.newIndex;
				continue;
			}
			if (candidate.kind === 'added') {
				lastNewIndex = candidate.newIndex;
				if (firstChangeNew === null) {
					firstChangeNew = candidate.newIndex;
				}
			}
			if (candidate.kind === 'removed' && firstChangeNew === null) {
				firstChangeNew = lastNewIndex;
			}
		}
		row.scopeLine = findScope(oldLines, newLines, language, firstChangeNew ?? row.firstNewIndex);
	});
}

/**
 * Pairs removed/added rows side by side for a split layout, filling with
 * `null` on whichever side runs out first in an uneven block (port of the
 * playground's split loop). Context, collapsed, and recollapse rows pass
 * through unchanged (a context row renders as both halves of one row; a
 * collapsed/recollapse row spans the full width).
 */
export function toSplitRows(rows: readonly DiffRow[]): SplitViewRow[] {
	const result: SplitViewRow[] = [];
	let index = 0;
	while (index < rows.length) {
		const row = rows[index];
		if (row.kind === 'collapsed' || row.kind === 'recollapse') {
			result.push({ left: row, right: null, full: true });
			index++;
			continue;
		}
		if (row.kind === 'context') {
			result.push({ left: row, right: row, full: false });
			index++;
			continue;
		}
		const removed: DiffRow[] = [];
		const added: DiffRow[] = [];
		while (index < rows.length && rows[index].kind === 'removed') {
			removed.push(rows[index++]);
		}
		while (index < rows.length && rows[index].kind === 'added') {
			added.push(rows[index++]);
		}
		const pairCount = Math.max(removed.length, added.length);
		for (let pairIndex = 0; pairIndex < pairCount; pairIndex++) {
			result.push({ left: removed[pairIndex] ?? null, right: added[pairIndex] ?? null, full: false });
		}
	}
	return result;
}
