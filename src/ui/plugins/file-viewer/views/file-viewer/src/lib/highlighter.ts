import hljs from 'highlight.js/lib/core';
import typescript from 'highlight.js/lib/languages/typescript';
import javascript from 'highlight.js/lib/languages/javascript';
import python from 'highlight.js/lib/languages/python';
import rust from 'highlight.js/lib/languages/rust';
import nix from 'highlight.js/lib/languages/nix';
import json from 'highlight.js/lib/languages/json';
import yaml from 'highlight.js/lib/languages/yaml';
import bash from 'highlight.js/lib/languages/bash';
import sql from 'highlight.js/lib/languages/sql';
import go from 'highlight.js/lib/languages/go';
import css from 'highlight.js/lib/languages/css';
import markdown from 'highlight.js/lib/languages/markdown';
import dockerfile from 'highlight.js/lib/languages/dockerfile';
import xml from 'highlight.js/lib/languages/xml';
import ini from 'highlight.js/lib/languages/ini';

// Register languages
hljs.registerLanguage('typescript', typescript);
hljs.registerLanguage('javascript', javascript);
hljs.registerLanguage('python', python);
hljs.registerLanguage('rust', rust);
hljs.registerLanguage('nix', nix);
hljs.registerLanguage('json', json);
hljs.registerLanguage('yaml', yaml);
hljs.registerLanguage('bash', bash);
hljs.registerLanguage('sql', sql);
hljs.registerLanguage('go', go);
hljs.registerLanguage('css', css);
hljs.registerLanguage('markdown', markdown);
hljs.registerLanguage('dockerfile', dockerfile);
hljs.registerLanguage('xml', xml);
hljs.registerLanguage('ini', ini);
hljs.registerLanguage('toml', ini);

/**
 * Highlights code using highlight.js
 * @param code The code string to highlight
 * @param language Optional language hint (typescript, python, rust, etc.)
 * @returns HTML string with syntax highlighting
 */
export function highlightCode(code: string, language?: string): string {
	if (!code) {
		return '';
	}

	try {
		if (language) {
			// Use the provided language hint
			return hljs.highlight(code, { language, ignoreIllegals: true }).value;
		} else {
			// Fall back to auto-detection
			return hljs.highlightAuto(code).value;
		}
	} catch {
		// On error, return the code unhighlighted
		return escapeHtml(code);
	}
}

/**
 * Detects the best language for a code string
 * @param code The code string to analyze
 * @returns The detected language name, or undefined if detection fails
 */
export function detectLanguage(code: string): string | undefined {
	if (!code) {
		return undefined;
	}

	try {
		const result = hljs.highlightAuto(code);
		return result.language;
	} catch {
		return undefined;
	}
}

/**
 * Escapes HTML special characters
 * @param text The text to escape
 * @returns Escaped HTML
 */
export function escapeHtml(text: string): string {
	const map: Record<string, string> = {
		'&': '&amp;',
		'<': '&lt;',
		'>': '&gt;',
		'"': '&quot;',
		"'": '&#039;'
	};
	return text.replace(/[&<>"']/g, (char) => map[char]);
}

export interface MatchRange {
	start: number;
	end: number;
}

function sameRange(a: MatchRange, b: MatchRange | null): boolean {
	return !!b && a.start === b.start && a.end === b.end;
}

/**
 * Splits `line` into segments at `matches`' boundaries and highlights each
 * segment INDEPENDENTLY via `highlightCode`, wrapping matched segments in
 * `<mark class="search-match">` (plus `search-match-current` for the one
 * equal to `currentMatch`). This is a deliberate per-segment approach rather
 * than string-splicing `<mark>` into a single whole-line `highlightCode`
 * result: splicing risks landing inside an already-generated `<span>` and
 * breaking the markup. The accepted, documented trade-off is that a match
 * falling inside a multi-character token may highlight that token's syntax
 * color slightly differently right at the boundary, because each segment is
 * highlighted without the surrounding line's context — cosmetic only, never
 * affecting which text is reported as matched.
 */
export function highlightLineWithMatches(
	line: string,
	language: string | undefined,
	matches: MatchRange[],
	currentMatch: MatchRange | null,
): string {
	if (matches.length === 0) {
		return highlightCode(line, language);
	}

	let html = '';
	let cursor = 0;
	for (const match of matches) {
		if (match.start > cursor) {
			html += highlightCode(line.slice(cursor, match.start), language);
		}
		const matchText = line.slice(match.start, match.end);
		const cssClass = sameRange(match, currentMatch) ? 'search-match search-match-current' : 'search-match';
		html += `<mark class="${cssClass}">${escapeHtml(matchText)}</mark>`;
		cursor = match.end;
	}
	if (cursor < line.length) {
		html += highlightCode(line.slice(cursor), language);
	}
	return html;
}

/**
 * The plain-text equivalent of `highlightLineWithMatches`: no syntax
 * highlighting, every non-matched segment is HTML-escaped verbatim, matched
 * segments are escaped and wrapped in `<mark class="search-match">` (plus
 * `search-match-current` for the one equal to `currentMatch`).
 */
export function highlightPlainLineWithMatches(
	line: string,
	matches: MatchRange[],
	currentMatch: MatchRange | null = null,
): string {
	if (matches.length === 0) {
		return escapeHtml(line);
	}

	let html = '';
	let cursor = 0;
	for (const match of matches) {
		if (match.start > cursor) {
			html += escapeHtml(line.slice(cursor, match.start));
		}
		const matchText = line.slice(match.start, match.end);
		const cssClass = sameRange(match, currentMatch) ? 'search-match search-match-current' : 'search-match';
		html += `<mark class="${cssClass}">${escapeHtml(matchText)}</mark>`;
		cursor = match.end;
	}
	if (cursor < line.length) {
		html += escapeHtml(line.slice(cursor));
	}
	return html;
}
