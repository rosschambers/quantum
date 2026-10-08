import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest';
import { render, cleanup } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import MarkdownRenderer from './MarkdownRenderer.svelte';

function rect(partial: Partial<DOMRect>): DOMRect {
    return {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        toJSON() {
            return this;
        },
        ...partial,
    } as DOMRect;
}

vi.mock('mermaid', () => ({ default: {
    initialize: vi.fn(),
    render: vi.fn(async () => ({ svg: '<svg><text>needle diagram</text></svg>' })),
} }));

beforeAll(() => {
    if (typeof Element.prototype.scrollIntoView !== 'function') {
        Element.prototype.scrollIntoView = function (): void {};
    }
    if (typeof (globalThis as any).ResizeObserver === 'undefined') {
        (globalThis as any).ResizeObserver = class {
            observe(): void {}
            unobserve(): void {}
            disconnect(): void {}
        };
    }
});

afterEach(() => {
    cleanup();
});

describe('MarkdownRenderer search highlighting', () => {
    it('preserves authored search-match marks on initial mount and through query changes and clearing', async () => {
        const content = '<p>Before <mark class="search-match" data-original="true">authored needle</mark> after</p>';
        const { container, rerender } = render(MarkdownRenderer, { props: { content } });
        const root = container.querySelector('.markdown-renderer')!;
        const authoredMark = root.querySelector('mark[data-original="true"]')!;
        expect(authoredMark).not.toBeNull();
        const originalHtml = root.innerHTML;
        const clicked = vi.fn();
        authoredMark.addEventListener('click', clicked);
        for (const query of ['needle', 'authored needle', '']) {
            await rerender({ query, currentMatchIndex: query ? 0 : null });
            expect(root.querySelector('mark[data-original="true"]')).toBe(authoredMark);
            expect(authoredMark.getAttribute('class')).toBe('search-match');
            expect(authoredMark.textContent).toBe('authored needle');
            authoredMark.dispatchEvent(new Event('click'));
        }
        expect(root.innerHTML).toBe(originalHtml);
        expect(clicked).toHaveBeenCalledTimes(3);
        expect(root.querySelectorAll('mark')).toHaveLength(1);
    });

    it('preserves original inline elements and attributes across cross-node search cycles', async () => {
        const content = '[prefix needle](/target "Original title") suffix';
        const counts = vi.fn();
        const { container, rerender } = render(MarkdownRenderer, { props: { content, onMatchCount: counts } });
        const root = container.querySelector('.markdown-renderer')!;
        const originalHtml = root.innerHTML;
        const originalLink = root.querySelector('a')!;
        const clicked = vi.fn();
        originalLink.addEventListener('click', clicked);
        await rerender({ content, query: 'needle suffix', currentMatchIndex: 0, onMatchCount: counts });
        expect(counts).toHaveBeenLastCalledWith(1);
        expect(root.querySelectorAll('a')).toHaveLength(1);
        expect(root.querySelector('a')).toBe(originalLink);
        expect(Array.from(root.querySelectorAll('mark')).map((mark) => mark.textContent).join('')).toBe('needle suffix');
        await rerender({ content, query: '', onMatchCount: counts });
        expect(root.innerHTML).toBe(originalHtml);
        expect(root.querySelector('a')).toBe(originalLink);
        originalLink.dispatchEvent(new Event('click'));
        expect(clicked).toHaveBeenCalledOnce();
    });

    it('does not report a new count when only the current selection changes', async () => {
        const counts = vi.fn();
        const { rerender } = render(MarkdownRenderer, { props: { content: 'cat cat', query: 'cat', currentMatchIndex: 0, onMatchCount: counts } });
        counts.mockClear();
        await rerender({ currentMatchIndex: 1 });
        expect(counts).not.toHaveBeenCalled();
        await rerender({ navigationRevision: 1 });
        expect(counts).not.toHaveBeenCalled();
    });
    it('highlights a matched substring with a search-match mark', async () => {
        const { container } = render(MarkdownRenderer, {
            props: { content: 'Find the needle in the haystack.', query: 'needle' },
        });
        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')?.textContent?.toLowerCase()).toBe('needle');
        });
    });

    it('finds a match spanning an inline element boundary (bold text)', async () => {
        const { container } = render(MarkdownRenderer, {
            props: { content: 'nee**dle** in haystack', query: 'needle' },
        });
        await vi.waitFor(() => {
            const marks = container.querySelectorAll('mark.search-match');
            expect(marks.length).toBeGreaterThan(0);
            expect(Array.from(marks).map((mark) => mark.textContent).join('').toLowerCase()).toBe('needle');
        });
    });

    it('never highlights diagram source inside a mermaid fence', async () => {
        const content = '```mermaid\ngraph TD;\n  needle --> B;\n```\n\nFind the needle in prose.';
        const { container } = render(MarkdownRenderer, {
            props: { content, query: 'needle' },
        });
        await vi.waitFor(() => {
            expect(container.querySelectorAll('mark.search-match')).toHaveLength(1);
        });
        const diagramBlock = container.querySelector('.diagram-block');
        await vi.waitFor(() => expect(diagramBlock?.querySelector('svg')?.textContent).toBe('needle diagram'));
        expect(diagramBlock?.querySelector('mark.search-match')).toBeNull();
    });

    it('reports the match count via onMatchCount', async () => {
        const counts: number[] = [];
        render(MarkdownRenderer, {
            props: {
                content: 'cat cat cat',
                query: 'cat',
                onMatchCount: (count: number) => counts.push(count),
            },
        });
        await vi.waitFor(() => expect(counts.at(-1)).toBe(3));
    });

    it('marks the current match with an additional class', async () => {
        const { container } = render(MarkdownRenderer, {
            props: { content: 'cat cat cat', query: 'cat', currentMatchIndex: 1 },
        });
        await vi.waitFor(() => {
            expect(container.querySelectorAll('mark.search-match')).toHaveLength(3);
        });
        const marks = container.querySelectorAll('mark.search-match');
        expect(marks[1].classList.contains('search-match-current')).toBe(true);
        expect(marks[0].classList.contains('search-match-current')).toBe(false);
    });

    it('restores the original DOM (no marks, text intact) when the query is cleared', async () => {
        const { container, rerender } = render(MarkdownRenderer, {
            props: { content: 'Find the needle in the haystack.', query: 'needle' },
        });
        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')).not.toBeNull();
        });

        await rerender({ content: 'Find the needle in the haystack.', query: '' });

        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')).toBeNull();
        });
        // marked's own paragraph renderer appends a trailing newline inside
        // the <p> text content; trim to compare the meaningful text only.
        expect(container.querySelector('.markdown-renderer')?.textContent?.trim()).toBe('Find the needle in the haystack.');
    });

    it('protects existing headings: the anchor-heading structure survives a search cycle', async () => {
        const { container, rerender } = render(MarkdownRenderer, {
            props: { content: '# Needle Heading\n\nSome needle text.', query: 'needle' },
        });
        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')).not.toBeNull();
        });

        const heading = container.querySelector('h1.anchor-heading');
        expect(heading).not.toBeNull();
        expect(heading?.querySelector('a.anchor-link')).not.toBeNull();

        await rerender({ content: '# Needle Heading\n\nSome needle text.', query: '' });

        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')).toBeNull();
        });
        const headingAfter = container.querySelector('h1.anchor-heading');
        expect(headingAfter).not.toBeNull();
        expect(headingAfter?.querySelector('a.anchor-link')).not.toBeNull();
        expect(headingAfter?.textContent).toContain('Needle Heading');
    });

    it('is case-insensitive', async () => {
        const { container } = render(MarkdownRenderer, {
            props: { content: 'NEEDLE in a haystack', query: 'needle' },
        });
        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')).not.toBeNull();
        });
    });

    it('does not render any mark when the query is empty', async () => {
        const { container } = render(MarkdownRenderer, {
            props: { content: 'Find the needle in the haystack.', query: '' },
        });
        expect(container.querySelector('mark.search-match')).toBeNull();
    });

    it('wraps many matches without a quadratic node scan', async () => {
        const content = Array.from({ length: 300 }, (_, index) => `Paragraph ${index}${index % 2 === 0 ? ' token' : ''} *tail*`).join('\n\n');
        const counts: number[] = [];
        const { container, rerender } = render(MarkdownRenderer, { content, query: '', onMatchCount: (count: number) => counts.push(count) });
        const started = performance.now();
        await rerender({ content, query: 'token', onMatchCount: (count: number) => counts.push(count) });
        await tick();
        expect(performance.now() - started).toBeLessThan(1000);
        expect(counts.at(-1)).toBe(150);
        expect(container.querySelectorAll('mark.search-match')).toHaveLength(150);
    });

    it('measures match positions from each match\'s first mark, scheduled a frame after the search pass', async () => {
        const frameCallbacks: FrameRequestCallback[] = [];
        let nextFrameId = 1;
        vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
            frameCallbacks.push(callback);
            return nextFrameId++;
        });
        vi.stubGlobal('cancelAnimationFrame', () => {});
        const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
            if (this.classList.contains('markdown-renderer')) return rect({ top: 0, height: 1000 });
            return rect({ top: 250, height: 20 });
        });
        try {
            const positionCalls: Float64Array[] = [];
            render(MarkdownRenderer, {
                props: { content: 'Find the needle in prose.', query: 'needle', onMatchPositions: (positions: Float64Array) => positionCalls.push(positions) },
            });
            await vi.waitFor(() => expect(frameCallbacks.length).toBeGreaterThan(0));
            for (const callback of frameCallbacks.splice(0)) callback(0);
            expect(positionCalls.at(-1)).toEqual(new Float64Array([250 / 1000]));
        } finally {
            rectSpy.mockRestore();
            vi.unstubAllGlobals();
        }
    });

    it('re-measures match positions when the content root resizes', async () => {
        let resizeCallback: ResizeObserverCallback | undefined;
        vi.stubGlobal(
            'ResizeObserver',
            class {
                constructor(callback: ResizeObserverCallback) {
                    resizeCallback = callback;
                }
                observe(): void {}
                unobserve(): void {}
                disconnect(): void {}
            },
        );
        vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
            callback(0);
            return 1;
        });
        vi.stubGlobal('cancelAnimationFrame', () => {});
        let anchorTop = 100;
        const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
            if (this.classList.contains('markdown-renderer')) return rect({ top: 0, height: 1000 });
            return rect({ top: anchorTop, height: 20 });
        });
        try {
            const positionCalls: Float64Array[] = [];
            render(MarkdownRenderer, {
                props: { content: 'Find the needle in prose.', query: 'needle', onMatchPositions: (positions: Float64Array) => positionCalls.push(positions) },
            });
            await vi.waitFor(() => expect(positionCalls.length).toBeGreaterThan(0));
            expect(positionCalls.at(-1)).toEqual(new Float64Array([100 / 1000]));

            anchorTop = 300;
            positionCalls.length = 0;
            resizeCallback?.([], {} as ResizeObserver);
            expect(positionCalls.at(-1)).toEqual(new Float64Array([300 / 1000]));
        } finally {
            rectSpy.mockRestore();
            vi.unstubAllGlobals();
        }
    });

    it('re-measures match positions after the mermaid render loop completes, without calling onMatchCount again', async () => {
        vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
            callback(0);
            return 1;
        });
        vi.stubGlobal('cancelAnimationFrame', () => {});
        const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
            if (this.classList.contains('markdown-renderer')) return rect({ top: 0, height: 2000 });
            return rect({ top: 1500, height: 20 });
        });
        try {
            const content = '```mermaid\ngraph TD;\n  A --> B;\n```\n\nFind the needle in prose.';
            const positionCalls: Float64Array[] = [];
            const counts: number[] = [];
            const { container } = render(MarkdownRenderer, {
                props: {
                    content,
                    query: 'needle',
                    onMatchPositions: (positions: Float64Array) => positionCalls.push(positions),
                    onMatchCount: (count: number) => counts.push(count),
                },
            });
            await vi.waitFor(() => expect(container.querySelector('.diagram-block svg')?.textContent).toBe('needle diagram'));
            // The match itself ("needle" in the prose paragraph) is found by
            // the very first synchronous search pass regardless of whether
            // the diagram has rendered yet (diagram source is skipped by
            // findMatchSegmentsInDom either way) — one call, scheduled a
            // frame after that pass. A second call, beyond that one, proves
            // the mermaid render loop's own completion triggered its own
            // extra re-measurement.
            await vi.waitFor(() => expect(positionCalls.length).toBeGreaterThanOrEqual(2));
            expect(positionCalls.at(-1)).toEqual(new Float64Array([1500 / 2000]));
            // The diagram finishing never changes the match count itself.
            expect(new Set(counts)).toEqual(new Set([1]));
        } finally {
            rectSpy.mockRestore();
            vi.unstubAllGlobals();
        }
    });

    it('never calls onMatchPositions when only currentMatchIndex changes', async () => {
        vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
            callback(0);
            return 1;
        });
        vi.stubGlobal('cancelAnimationFrame', () => {});
        const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
            if (this.classList.contains('markdown-renderer')) return rect({ top: 0, height: 1000 });
            return rect({ top: 100, height: 20 });
        });
        try {
            const positionCalls: Float64Array[] = [];
            const onMatchPositions = (positions: Float64Array) => positionCalls.push(positions);
            const { rerender } = render(MarkdownRenderer, {
                props: { content: 'cat cat', query: 'cat', currentMatchIndex: 0, onMatchPositions },
            });
            await vi.waitFor(() => expect(positionCalls.length).toBeGreaterThan(0));
            positionCalls.length = 0;
            await rerender({ currentMatchIndex: 1 });
            expect(positionCalls).toHaveLength(0);
        } finally {
            rectSpy.mockRestore();
            vi.unstubAllGlobals();
        }
    });
});
