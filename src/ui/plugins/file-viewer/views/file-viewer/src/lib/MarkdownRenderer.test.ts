import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest';
import { render, cleanup } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import MarkdownRenderer from './MarkdownRenderer.svelte';

vi.mock('mermaid', () => ({ default: {
    initialize: vi.fn(),
    render: vi.fn(async () => ({ svg: '<svg><text>needle diagram</text></svg>' })),
} }));

beforeAll(() => {
    if (typeof Element.prototype.scrollIntoView !== 'function') {
        Element.prototype.scrollIntoView = function (): void {};
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
});
