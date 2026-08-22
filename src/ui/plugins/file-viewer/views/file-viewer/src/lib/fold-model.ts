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
