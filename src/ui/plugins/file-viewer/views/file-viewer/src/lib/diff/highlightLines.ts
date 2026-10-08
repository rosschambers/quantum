// Whole-file syntax highlighting split into per-line tokens, for diff mode.
// `highlightCode` (../highlighter.ts) tokenizes a whole string at once so
// multi-line constructs (block comments, template strings) color correctly
// — never per-line, which is how `CodeRenderer` highlighted before Task 3
// and exactly the bug the diff design doc calls out. This module walks that
// single HTML string once, tracking the open `<span class="…">` stack, and
// distributes the resulting tokens across source lines, carrying the stack
// across embedded newlines so a comment spanning three lines colors all
// three. `renderTokens` then re-assembles a line's tokens into HTML while
// splitting tokens at diff/search emphasis range boundaries.
import { highlightCode, escapeHtml } from '../highlighter';

export interface DiffToken {
	text: string;
	classes: string;
}

export interface EmphasisLayer {
	ranges: readonly [number, number][];
	className: string;
}

const SPAN_TAG_PATTERN = /<span class="([^"]*)">|<\/span>/g;

const ENTITY_PATTERN = /&amp;|&lt;|&gt;|&quot;|&#x27;|&#039;/g;
const ENTITY_MAP: Record<string, string> = {
	'&amp;': '&',
	'&lt;': '<',
	'&gt;': '>',
	'&quot;': '"',
	'&#x27;': "'",
	'&#039;': "'",
};

function decodeEntities(text: string): string {
	return text.replace(ENTITY_PATTERN, (entity) => ENTITY_MAP[entity]);
}

/**
 * Highlights `content` as a whole (never per line) and splits the result
 * into one token array per source line. Without a `language`, returns
 * plain tokens with no highlight classes — a wrong auto-detected language
 * is worse than no highlighting in a diff, so there is deliberately no
 * fallback to auto-detection here (unlike `highlightCode` itself).
 */
export function highlightLines(content: string, language: string | undefined): DiffToken[][] {
	const lineCount = content.length === 0 ? 1 : content.split('\n').length;

	if (!language) {
		return content.split('\n').map((line) => (line.length === 0 ? [] : [{ text: line, classes: '' }]));
	}

	const html = highlightCode(content, language);
	const lines: DiffToken[][] = [[]];
	const stack: string[] = [];

	function appendText(rawText: string): void {
		if (rawText.length === 0) {
			return;
		}
		const decoded = decodeEntities(rawText);
		const segments = decoded.split('\n');
		const classes = stack.join(' ');
		segments.forEach((segment, index) => {
			if (segment.length > 0) {
				lines[lines.length - 1].push({ text: segment, classes });
			}
			if (index < segments.length - 1) {
				lines.push([]);
			}
		});
	}

	let cursor = 0;
	let match: RegExpExecArray | null;
	SPAN_TAG_PATTERN.lastIndex = 0;
	while ((match = SPAN_TAG_PATTERN.exec(html))) {
		if (match.index > cursor) {
			appendText(html.slice(cursor, match.index));
		}
		if (match[0] === '</span>') {
			stack.pop();
		} else {
			stack.push(match[1]);
		}
		cursor = SPAN_TAG_PATTERN.lastIndex;
	}
	if (cursor < html.length) {
		appendText(html.slice(cursor));
	}

	// Highlighting never adds or removes newlines, but guard against a
	// mismatch rather than silently truncating or padding with the wrong
	// number of lines.
	while (lines.length < lineCount) {
		lines.push([]);
	}
	return lines;
}

/**
 * Re-assembles a line's tokens into an HTML string, wrapping each `layers`
 * entry's character ranges in a `<span class="layer.className">` — applied
 * by splitting tokens at the range boundaries, never by splicing into
 * already-generated HTML. Layers nest in array order (the first layer is
 * outermost); the highlight.js token class (if any) is always innermost.
 */
export function renderTokens(tokens: readonly DiffToken[], layers: readonly EmphasisLayer[]): string {
	let html = '';
	let offset = 0;
	const openLayers: boolean[] = layers.map(() => false);

	function activeMaskAt(position: number): boolean[] {
		return layers.map((layer) => layer.ranges.some(([start, end]) => position >= start && position < end));
	}

	function transitionTo(target: readonly boolean[]): void {
		let divergeIndex = layers.length;
		for (let index = 0; index < layers.length; index++) {
			if (openLayers[index] !== target[index]) {
				divergeIndex = index;
				break;
			}
		}
		for (let index = layers.length - 1; index >= divergeIndex; index--) {
			if (openLayers[index]) {
				html += '</span>';
				openLayers[index] = false;
			}
		}
		for (let index = divergeIndex; index < layers.length; index++) {
			if (target[index] && !openLayers[index]) {
				html += `<span class="${layers[index].className}">`;
				openLayers[index] = true;
			}
		}
	}

	for (const token of tokens) {
		let index = 0;
		while (index < token.text.length) {
			const position = offset + index;
			const mask = activeMaskAt(position);
			let end = index + 1;
			while (end < token.text.length) {
				const nextMask = activeMaskAt(offset + end);
				if (nextMask.some((value, layerIndex) => value !== mask[layerIndex])) {
					break;
				}
				end++;
			}
			transitionTo(mask);
			const segment = escapeHtml(token.text.slice(index, end));
			html += token.classes ? `<span class="${token.classes}">${segment}</span>` : segment;
			index = end;
		}
		offset += token.text.length;
	}
	transitionTo(layers.map(() => false));
	return html;
}
