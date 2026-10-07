// Pure, literal, case-insensitive substring search — no regular expressions,
// no replacement. One function per content type's notion of "text":
//   - Text/Code/JSON key everything by line (findMatchesInLines), against the
//     RAW pre-highlight line string, so offsets never have to account for
//     injected <span> markup.
//   - Markdown has no raw-text form of "what the user sees" — marked's HTML
//     output is the only place visible text exists — so findMatchesInDom
//     walks the rendered DOM instead.

export interface MatchRange {
    start: number;
    end: number;
}

/**
 * Literal, case-insensitive, non-overlapping substring search within a single
 * string. An empty query always yields no matches.
 */
export function findMatches(text: string, query: string): MatchRange[] {
    if (!query) {
        return [];
    }

    // Only whole original Unicode characters may delimit a match. Lowercase
    // expansion (such as U+0130) must not shift offsets or allow partial hits.
    const boundaries = new Map<number, number>([[0, 0]]);
    let lowerText = '';
    let originalOffset = 0;
    for (const character of text) {
        lowerText += character.toLowerCase();
        originalOffset += character.length;
        boundaries.set(lowerText.length, originalOffset);
    }
    const lowerQuery = Array.from(query, (character) => character.toLowerCase()).join('');
    const matches: MatchRange[] = [];

    let searchFrom = 0;
    while (searchFrom <= lowerText.length) {
        const index = lowerText.indexOf(lowerQuery, searchFrom);
        if (index === -1) {
            break;
        }
        const end = index + lowerQuery.length;
        const originalStart = boundaries.get(index);
        const originalEnd = boundaries.get(end);
        if (originalStart !== undefined && originalEnd !== undefined) {
            matches.push({ start: originalStart, end: originalEnd });
            searchFrom = end;
        } else {
            searchFrom = index + 1;
        }
    }

    return matches;
}

export interface LineMatch {
    lineIndex: number;
    range: MatchRange;
}

/**
 * Literal, case-insensitive search across an array of lines (the basis for
 * Text/Code/JSON search — each keys its rendering by line index already).
 * Matches are returned in document order: line by line, left to right within
 * each line.
 */
export function findMatchesInLines(lines: string[], query: string): LineMatch[] {
    if (!query) {
        return [];
    }

    const result: LineMatch[] = [];
    for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
        for (const range of findMatches(lines[lineIndex], query)) {
            result.push({ lineIndex, range });
        }
    }
    return result;
}

// Block-level tags whose boundary inserts a separator into the concatenated
// text used for DOM search. Two text nodes that are siblings-in-spirit only
// because they sit inside different block elements (two separate paragraphs,
// a heading followed by a list item, ...) must never be treated as adjacent
// — otherwise a search could match text that spans a visual line break the
// user would never read as contiguous. Inline elements (strong, em, a, code,
// span, ...) are deliberately NOT in this set: the design's explicit
// requirement is that a match may span an inline node boundary (for example
// "nee<strong>dle</strong>" must find "needle").
const BLOCK_TAGS = new Set([
    'P', 'DIV', 'SECTION', 'ARTICLE', 'HEADER', 'FOOTER', 'MAIN',
    'H1', 'H2', 'H3', 'H4', 'H5', 'H6',
    'LI', 'UL', 'OL', 'BLOCKQUOTE', 'PRE',
    'TABLE', 'THEAD', 'TBODY', 'TR', 'TD', 'TH', 'HR', 'BR',
]);

/** A rendered text node's contribution to the concatenated search string. */
interface TextSpan {
    node: Text;
    start: number;
    end: number;
}

function isDiagramElement(element: Element): boolean {
    return element.classList.contains('diagram-block') || element.classList.contains('diagram-error');
}

function collectTextSpans(root: Element): { text: string; spans: TextSpan[] } {
    let text = '';
    const spans: TextSpan[] = [];

    function walk(node: Node): void {
        if (node.nodeType === Node.TEXT_NODE) {
            const content = node.textContent ?? '';
            if (content.length === 0) {
                return;
            }
            const start = text.length;
            text += content;
            spans.push({ node: node as Text, start, end: start + content.length });
            return;
        }

        if (node.nodeType !== Node.ELEMENT_NODE) {
            return;
        }

        const element = node as Element;
        if (isDiagramElement(element)) {
            // Skip the whole subtree: diagram source/SVG is never searchable.
            return;
        }

        for (const child of Array.from(node.childNodes)) {
            walk(child);
        }

        if (BLOCK_TAGS.has(element.tagName) && text.length > 0 && !text.endsWith('\n')) {
            text += '\n';
        }
    }

    walk(root);
    return { text, spans };
}

function spanAt(spans: TextSpan[], offset: number, preferEnd: boolean): TextSpan | undefined {
    return spans.find((span) =>
        preferEnd ? offset > span.start && offset <= span.end : offset >= span.start && offset < span.end,
    );
}

function buildRange(spans: TextSpan[], start: number, end: number): Range | null {
    const startSpan = spanAt(spans, start, false);
    const endSpan = spanAt(spans, end, true);
    if (!startSpan || !endSpan) {
        return null;
    }
    const range = document.createRange();
    range.setStart(startSpan.node, start - startSpan.start);
    range.setEnd(endSpan.node, end - endSpan.start);
    return range;
}

/**
 * Literal, case-insensitive search over the RENDERED, VISIBLE text of a DOM
 * subtree (the basis for Markdown search). Walks every text node under
 * `root`, skipping any subtree rooted at an element with class
 * `diagram-block` or `diagram-error` (mermaid/graphviz source and SVG are
 * never searchable). A match may span an inline element boundary (for
 * example "nee<strong>dle</strong>") but never a block element boundary (two
 * separate paragraphs never merge into one match).
 */
export function findMatchesInDom(root: Element, query: string): Range[] {
    if (!query) {
        return [];
    }

    const { text, spans } = collectTextSpans(root);
    const ranges: Range[] = [];
    for (const { start, end } of findMatches(text, query)) {
        const range = buildRange(spans, start, end);
        if (range) {
            ranges.push(range);
        }
    }

    return ranges;
}
