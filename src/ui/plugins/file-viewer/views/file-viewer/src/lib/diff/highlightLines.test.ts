import { describe, test, expect } from 'vitest';
import { highlightLines, renderTokens, type DiffToken } from './highlightLines';

function plainText(tokens: DiffToken[]): string {
	return tokens.map((token) => token.text).join('');
}

describe('highlightLines', () => {
	test('without a language, returns plain tokens with no classes', () => {
		const lines = highlightLines('const a = 1;\nconst b = 2;', undefined);
		expect(lines).toHaveLength(2);
		expect(lines[0]).toEqual([{ text: 'const a = 1;', classes: '' }]);
		expect(lines[1]).toEqual([{ text: 'const b = 2;', classes: '' }]);
	});

	test('an empty line produces an empty token array, not a missing one', () => {
		const lines = highlightLines('a\n\nb', undefined);
		expect(lines).toHaveLength(3);
		expect(lines[1]).toEqual([]);
	});

	test('a block comment spanning three lines carries hljs-comment on every line', () => {
		const content = ['/* a multi', 'line comment', 'spanning three lines */', 'const x = 1;'].join('\n');
		const lines = highlightLines(content, 'typescript');
		expect(lines).toHaveLength(4);
		for (const line of lines.slice(0, 3)) {
			expect(line.length).toBeGreaterThan(0);
			for (const token of line) {
				expect(token.classes).toContain('hljs-comment');
			}
		}
		expect(plainText(lines[0])).toBe('/* a multi');
		expect(plainText(lines[1])).toBe('line comment');
		expect(plainText(lines[2])).toBe('spanning three lines */');
		// The line after the comment is not inside it.
		for (const token of lines[3]) {
			expect(token.classes).not.toContain('hljs-comment');
		}
	});

	test('round-trips through HTML entity decode and re-escape', () => {
		const content = 'const ok = a < b && c;';
		const lines = highlightLines(content, 'typescript');
		expect(lines).toHaveLength(1);
		expect(plainText(lines[0])).toBe(content);
		const html = renderTokens(lines[0], []);
		// escapeHtml must have re-escaped the decoded "<" and "&&" so the
		// rendered HTML is valid and round-trips back to the same text.
		expect(html).toContain('&lt;');
		expect(html).toContain('&amp;&amp;');
		const parsed = new DOMParser().parseFromString(`<div>${html}</div>`, 'text/html');
		expect(parsed.body.textContent).toBe(content);
	});
});

describe('renderTokens', () => {
	test('a layer range inside a single token splits it, both pieces keeping the token class', () => {
		const tokens: DiffToken[] = [{ text: 'extension_lower', classes: 'hljs-variable' }];
		const html = renderTokens(tokens, [{ ranges: [[0, 9]], className: 'ix-add' }]);
		expect(html).toBe(
			'<span class="ix-add"><span class="hljs-variable">extension</span></span><span class="hljs-variable">_lower</span>',
		);
	});

	test('two layers nest without breaking markup', () => {
		const tokens: DiffToken[] = [{ text: 'needle in a haystack', classes: '' }];
		const html = renderTokens(tokens, [
			{ ranges: [[0, 21]], className: 'ix-add' },
			{ ranges: [[0, 6]], className: 'search-match' },
		]);
		const parsed = new DOMParser().parseFromString(`<div>${html}</div>`, 'text/html');
		expect(parsed.body.textContent).toBe('needle in a haystack');
		const outer = parsed.body.querySelector('.ix-add');
		expect(outer).not.toBeNull();
		const inner = outer!.querySelector('.search-match');
		expect(inner).not.toBeNull();
		expect(inner!.textContent).toBe('needle');
	});

	test('escapes HTML special characters in the rendered text', () => {
		const html = renderTokens([{ text: 'a < b & "c"', classes: '' }], []);
		expect(html).toBe('a &lt; b &amp; &quot;c&quot;');
	});

	test('no layers and no classes renders plain escaped text', () => {
		const html = renderTokens([{ text: 'plain', classes: '' }], []);
		expect(html).toBe('plain');
	});
});
