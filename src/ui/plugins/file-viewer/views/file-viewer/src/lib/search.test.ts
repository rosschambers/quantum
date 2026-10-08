import { describe, test, expect } from 'vitest';
import { findMatches, findMatchesInLines, findMatchesInDom, findMatchSegmentsInDom } from './search';

describe('findMatches', () => {
    test('maps expanded lowercase characters back to original UTF16 offsets', () => {
        expect(findMatches('\u0130 cat', 'cat')).toEqual([{ start: 2, end: 5 }]);
        expect(findMatches('x \u0130cat', '\u0130CAT')).toEqual([{ start: 2, end: 6 }]);
        expect(findMatches('\u{1f600} \u0130 cat', 'cat')).toEqual([{ start: 5, end: 8 }]);
        expect(findMatches('\u0130', 'i')).toEqual([]);
        expect(findMatches('\u{1f600}', '\ud83d')).toEqual([]);
    });
    test('is case-insensitive and non-overlapping', () => {
        expect(findMatches('Foo foo', 'foo')).toEqual([{ start: 0, end: 3 }, { start: 4, end: 7 }]);
    });

    test('empty query returns no matches', () => {
        expect(findMatches('anything', '')).toEqual([]);
    });

    test('no matches returns an empty array', () => {
        expect(findMatches('hello world', 'xyz')).toEqual([]);
    });

    test('matches are literal, not a regular expression', () => {
        expect(findMatches('a.b.c', '.')).toEqual([{ start: 1, end: 2 }, { start: 3, end: 4 }]);
        expect(findMatches('a.b.c', 'a.b')).toEqual([{ start: 0, end: 3 }]);
    });

    test('adjacent non-overlapping matches', () => {
        expect(findMatches('aaaa', 'aa')).toEqual([{ start: 0, end: 2 }, { start: 2, end: 4 }]);
    });
});

describe('findMatchesInLines', () => {
    test('reports line index and in-line offsets, in order', () => {
        expect(findMatchesInLines(['alpha', 'beta alpha'], 'alpha')).toEqual([
            { lineIndex: 0, range: { start: 0, end: 5 } },
            { lineIndex: 1, range: { start: 5, end: 10 } },
        ]);
    });

    test('empty query returns no matches', () => {
        expect(findMatchesInLines(['alpha', 'beta'], '')).toEqual([]);
    });

    test('multiple matches on the same line all report', () => {
        expect(findMatchesInLines(['foo foo foo'], 'foo')).toEqual([
            { lineIndex: 0, range: { start: 0, end: 3 } },
            { lineIndex: 0, range: { start: 4, end: 7 } },
            { lineIndex: 0, range: { start: 8, end: 11 } },
        ]);
    });
});

describe('findMatchesInDom', () => {
    test('uses original offsets after and within expanded Unicode matches', () => {
        const root = document.createElement('div');
        root.innerHTML = '<p>\u{1f600} \u0130 cat <strong>\u0130</strong>cat</p>';
        expect(findMatchesInDom(root, 'cat').map((range) => range.toString())).toEqual(['cat', 'cat']);
        expect(findMatchesInDom(root, '\u0130CAT').map((range) => range.toString())).toEqual(['\u0130cat']);
    });

    test('an explicit line break separates rendered words', () => {
        const root = document.createElement('div');
        root.innerHTML = '<p>hello<br>world</p>';
        expect(findMatchesInDom(root, 'helloworld')).toHaveLength(0);
        expect(findMatchesInDom(root, 'world')[0].toString()).toBe('world');
    });
    test('skips diagram-block subtrees', () => {
        document.body.innerHTML = '<div id="r"><p>needle</p><div class="diagram-block">needle</div></div>';
        expect(findMatchesInDom(document.getElementById('r')!, 'needle')).toHaveLength(1);
    });

    test('skips diagram-error subtrees', () => {
        document.body.innerHTML = '<div id="r"><p>needle</p><div class="diagram-error">needle</div></div>';
        expect(findMatchesInDom(document.getElementById('r')!, 'needle')).toHaveLength(1);
    });

    test('finds a match that spans an inline element boundary (important)', () => {
        document.body.innerHTML = '<div id="r"><p>nee<strong>dle</strong> in haystack</p></div>';
        const ranges = findMatchesInDom(document.getElementById('r')!, 'needle');
        expect(ranges).toHaveLength(1);
        expect(ranges[0].toString().toLowerCase()).toBe('needle');
    });

    test('is case-insensitive', () => {
        document.body.innerHTML = '<div id="r"><p>NEEDLE in a haystack</p></div>';
        expect(findMatchesInDom(document.getElementById('r')!, 'needle')).toHaveLength(1);
    });

    test('does not merge text across separate block elements', () => {
        document.body.innerHTML = '<div id="r"><p>hello</p><p>world</p></div>';
        expect(findMatchesInDom(document.getElementById('r')!, 'oworld')).toHaveLength(0);
        expect(findMatchesInDom(document.getElementById('r')!, 'hello')).toHaveLength(1);
        expect(findMatchesInDom(document.getElementById('r')!, 'world')).toHaveLength(1);
    });

    test('empty query returns no matches', () => {
        document.body.innerHTML = '<div id="r"><p>needle</p></div>';
        expect(findMatchesInDom(document.getElementById('r')!, '')).toEqual([]);
    });

    test('finds multiple matches in order', () => {
        document.body.innerHTML = '<div id="r"><p>cat cat cat</p></div>';
        expect(findMatchesInDom(document.getElementById('r')!, 'cat')).toHaveLength(3);
    });
});

function domFrom(html: string): HTMLElement {
    const root = document.createElement('div');
    root.innerHTML = html;
    return root;
}

describe('findMatchSegmentsInDom', () => {
    test('returns one segment per text node a match covers, sharing the match index', () => {
        const root = domFrom('<p>nee<strong>dle</strong> and needle</p>');
        const result = findMatchSegmentsInDom(root, 'needle');
        expect(result.count).toBe(2);
        expect(result.segments.map((segment) => [segment.node.data.slice(segment.start, segment.end), segment.matchIndex]))
            .toEqual([['nee', 0], ['dle', 0], ['needle', 1]]);
    });

    test('never matches across a block boundary and skips diagram source', () => {
        const root = domFrom('<p>nee</p><p>dle</p><div class="diagram-block">needle</div>');
        expect(findMatchSegmentsInDom(root, 'needle').count).toBe(0);
    });

    test('has the same count as findMatchesInDom', () => {
        const root = domFrom('<h1>Upload</h1><p>up <em>upl</em>oad upload</p><ul><li>UPLOAD</li></ul>');
        for (const query of ['u', 'up', 'upload', 'oad u']) {
            expect(findMatchSegmentsInDom(root, query).count).toBe(findMatchesInDom(root, query).length);
        }
    });

    test('stays linear: 300 paragraphs with 150 matches finish quickly', () => {
        const html = Array.from({ length: 300 }, (_, index) => `<p>line ${index}${index % 2 === 0 ? ' token' : ''} <em>tail</em></p>`).join('');
        const root = domFrom(html);
        const started = performance.now();
        const result = findMatchSegmentsInDom(root, 'token');
        expect(result.count).toBe(150);
        expect(performance.now() - started).toBeLessThan(500);
    });
});
