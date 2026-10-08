export interface CodeFoldRange {
	startLine: number;
	endLine: number;
}

function indentLevel(line: string): number {
	const match = line.match(/^(\s*)/);
	return match ? match[1].length : 0;
}

function isBlank(line: string): boolean {
	return line.trim().length === 0;
}

/**
 * Every fold in `model` whose range strictly contains `line` — that is,
 * `fold.startLine < line <= fold.endLine` — sorted outermost first (ascending
 * by `startLine`). A line that is itself a fold's own header line
 * (`line === fold.startLine`) never counts as contained by that fold: a
 * fold's header is always visible regardless of its own collapsed state.
 *
 * Used by the search-reveal path to expand every enclosing fold, not just
 * the innermost one: `JsonFoldRenderer` and `CodeRenderer`'s non-virtual
 * line-walk both jump straight from a collapsed fold's start line to its
 * `endLine + 1`, skipping everything (including any folds nested inside it)
 * — so revealing a line nested several folds deep requires expanding every
 * ancestor, not only the tightest one, or the outer fold's collapsed state
 * alone keeps the line out of the render entirely.
 */
export function foldAncestors(model: Map<number, CodeFoldRange>, line: number): CodeFoldRange[] {
	const ancestors: CodeFoldRange[] = [];
	for (const fold of model.values()) {
		if (fold.startLine < line && line <= fold.endLine) {
			ancestors.push(fold);
		}
	}
	ancestors.sort((a, b) => a.startLine - b.startLine);
	return ancestors;
}

/**
 * The innermost (tightest) fold containing `line`, or `null` if none. See
 * `foldAncestors` for the full ancestor chain when a line may be nested
 * inside multiple folds.
 */
export function foldContaining(model: Map<number, CodeFoldRange>, line: number): CodeFoldRange | null {
	const ancestors = foldAncestors(model, line);
	if (ancestors.length === 0) {
		return null;
	}
	return ancestors[ancestors.length - 1];
}

/**
 * Maps every source line index to the "visible row" it renders at once folds
 * collapse. Walks identically to `CodeRenderer.svelte`'s non-virtual render
 * loop: a collapsed fold contributes exactly one row (its header line) and
 * every line it hides — including any folds nested inside it, regardless of
 * their own collapsed state — maps to that same header row, because the walk
 * jumps straight from the fold's start line to `endLine + 1` without
 * recursing into what is hidden. An expanded fold hides nothing: its header
 * is just another row, and the walk continues line by line into its body.
 *
 * Used to translate a search match's source line into the row the shared
 * overview ruler should place its mark at (`rowCenterFraction` takes the row,
 * not the raw line, so a match inside a collapsed fold marks the header).
 */
export function visibleRowOfLine(
	lineCount: number,
	model: Map<number, CodeFoldRange>,
	collapsed: (startLine: number) => boolean,
): { rowOfLine: Int32Array; rowCount: number } {
	const rowOfLine = new Int32Array(lineCount);
	let line = 0;
	let row = 0;
	while (line < lineCount) {
		const fold = model.get(line);
		if (fold && collapsed(line)) {
			for (let hidden = line; hidden <= fold.endLine; hidden++) {
				rowOfLine[hidden] = row;
			}
			row++;
			line = fold.endLine + 1;
		} else {
			rowOfLine[line] = row;
			row++;
			line++;
		}
	}
	return { rowOfLine, rowCount: row };
}

export function buildCodeFoldModel(lines: string[]): Map<number, CodeFoldRange> {
	const folds = new Map<number, CodeFoldRange>();

	for (let i = 0; i < lines.length; i++) {
		if (isBlank(lines[i])) continue;

		const currentIndent = indentLevel(lines[i]);

		// Find the next non-blank line
		let nextNonBlank = -1;
		for (let j = i + 1; j < lines.length; j++) {
			if (!isBlank(lines[j])) {
				nextNonBlank = j;
				break;
			}
		}

		if (nextNonBlank === -1) continue;
		if (indentLevel(lines[nextNonBlank]) <= currentIndent) continue;

		// This line starts a fold. Find where the block ends.
		let endLine = i;
		for (let j = i + 1; j < lines.length; j++) {
			if (isBlank(lines[j])) continue;
			if (indentLevel(lines[j]) <= currentIndent) break;
			endLine = j;
		}

		if (endLine > i) {
			folds.set(i, { startLine: i, endLine });
		}
	}

	return folds;
}
