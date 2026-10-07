import { describe, it, expect, beforeAll, vi } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import TextRenderer from './TextRenderer.svelte';
import CodeRenderer from './CodeRenderer.svelte';

beforeAll(() => {
    if (typeof (globalThis as any).ResizeObserver === 'undefined') {
        (globalThis as any).ResizeObserver = class {
            observe(): void {}
            unobserve(): void {}
            disconnect(): void {}
        };
    }
    if (typeof Element.prototype.scrollIntoView !== 'function') {
        Element.prototype.scrollIntoView = function (): void {};
    }
});

describe('TextRenderer search highlighting (non-virtualized)', () => {
    it('highlights a matched substring with a search-match mark', () => {
        const { container } = render(TextRenderer, {
            props: { content: 'find the needle here', query: 'needle' },
        });
        expect(container.querySelector('mark.search-match')?.textContent).toBe('needle');
    });

    it('does not render any mark when the query is empty, and preserves exact text', () => {
        const { container } = render(TextRenderer, {
            props: { content: 'find the needle here', query: '' },
        });
        expect(container.querySelector('mark.search-match')).toBeNull();
        expect(container.querySelector('.text-content')?.textContent).toBe('find the needle here');
    });

    it('never corrupts HTML-significant characters in unmatched text', () => {
        const { container } = render(TextRenderer, {
            props: { content: '<b>needle</b>', query: 'needle' },
        });
        expect(container.querySelector('.text-content')?.innerHTML).toContain('&lt;b&gt;');
        expect(container.querySelector('mark.search-match')?.textContent).toBe('needle');
    });

    it('reports the match count via onMatchCount', async () => {
        const counts: number[] = [];
        render(TextRenderer, {
            props: { content: 'cat cat cat', query: 'cat', onMatchCount: (count: number) => counts.push(count) },
        });
        await vi.waitFor(() => expect(counts.at(-1)).toBe(3));
    });

    it('marks the current match with an additional class', () => {
        const { container } = render(TextRenderer, {
            props: { content: 'cat cat cat', query: 'cat', currentMatchIndex: 2 },
        });
        const marks = container.querySelectorAll('mark.search-match');
        expect(marks).toHaveLength(3);
        expect(marks[2].classList.contains('search-match-current')).toBe(true);
        expect(marks[0].classList.contains('search-match-current')).toBe(false);
    });
});

describe('TextRenderer search highlighting (virtualized, >500 lines)', () => {
    it.each([TextRenderer, CodeRenderer])('uses exact block row geometry for virtual text and code including empty and long lines', async (component) => {
        const lines = Array.from({ length: 1000 }, (_, index) => index === 900 ? 'needle' : index % 2 ? '' : 'long '.repeat(200));
        const { container, rerender } = render(component, { props: { content: lines.join('\n') } });
        const scroller = container.querySelector('.virtual-scroller') as HTMLElement;
        for (const query of ['', 'needle']) {
            await rerender({ query, currentMatchIndex: query ? 0 : null });
            const rows = scroller.querySelectorAll<HTMLElement>('.text-line, .code-line');
            expect(rows.length).toBeGreaterThan(0);
            for (const row of rows) {
                expect(row.style.display).toBe('block');
                expect(row.style.height).toBe('21px');
                expect(row.style.lineHeight).toBe('21px');
                expect(row.style.whiteSpace).toBe('pre');
                expect(row.style.boxSizing).toBe('border-box');
            }
            const gutterRow = scroller.querySelector<HTMLElement>('.line-number');
            if (gutterRow) expect(gutterRow.style.height).toBe('21px');
        }
        expect(scroller.scrollTop).toBe(900 * 21);
    });
    it('reveals an offscreen match by scrolling the virtual window to include it', async () => {
        const lines = Array.from({ length: 600 }, (_, i) => (i === 550 ? 'the needle is here' : `line number ${i}`));
        const { container } = render(TextRenderer, {
            props: { content: lines.join('\n'), query: 'needle', currentMatchIndex: 0 },
        });

        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')).not.toBeNull();
        });
        expect(container.querySelector('mark.search-match')?.textContent).toBe('needle');
    });
});
