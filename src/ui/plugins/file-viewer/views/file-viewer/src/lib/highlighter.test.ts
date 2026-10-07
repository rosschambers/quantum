import { describe, test, expect } from 'vitest';
import { highlightLineWithMatches, highlightPlainLineWithMatches } from './highlighter';

describe('highlightLineWithMatches', () => {
    test('with no matches, behaves exactly like highlightCode (no mark wrapping)', () => {
        const result = highlightLineWithMatches('const x = 1;', 'javascript', [], null);
        expect(result).not.toContain('<mark');
        expect(result).toContain('x');
    });

    test('wraps a matched segment in a mark, highlighting the rest normally', () => {
        const result = highlightLineWithMatches('const needle = 1;', undefined, [{ start: 6, end: 12 }], null);
        expect(result).toContain('<mark class="search-match">needle</mark>');
    });

    test('marks the current match with an additional class', () => {
        const result = highlightLineWithMatches('const needle = 1;', undefined, [{ start: 6, end: 12 }], { start: 6, end: 12 });
        expect(result).toContain('<mark class="search-match search-match-current">needle</mark>');
    });

    test('never corrupts the exact match text even across multiple matches on one line', () => {
        const result = highlightLineWithMatches('cat cat cat', undefined, [
            { start: 0, end: 3 }, { start: 4, end: 7 }, { start: 8, end: 11 },
        ], { start: 4, end: 7 });
        expect(result).toBe(
            '<mark class="search-match">cat</mark> <mark class="search-match search-match-current">cat</mark> <mark class="search-match">cat</mark>',
        );
    });

    test('a match containing HTML-significant characters is escaped, never injected raw', () => {
        const result = highlightLineWithMatches('<script>', undefined, [{ start: 0, end: 8 }], null);
        expect(result).toBe('<mark class="search-match">&lt;script&gt;</mark>');
    });
});

describe('highlightPlainLineWithMatches', () => {
    test('with no matches, escapes the line like plain text (no mark wrapping)', () => {
        const result = highlightPlainLineWithMatches('<b>hello</b>', []);
        expect(result).toBe('&lt;b&gt;hello&lt;/b&gt;');
    });

    test('wraps a matched segment in a mark', () => {
        const result = highlightPlainLineWithMatches('find the needle here', [{ start: 9, end: 15 }], null);
        expect(result).toBe('find the <mark class="search-match">needle</mark> here');
    });

    test('marks the current match with an additional class', () => {
        const result = highlightPlainLineWithMatches('find the needle here', [{ start: 9, end: 15 }], { start: 9, end: 15 });
        expect(result).toBe('find the <mark class="search-match search-match-current">needle</mark> here');
    });
});
