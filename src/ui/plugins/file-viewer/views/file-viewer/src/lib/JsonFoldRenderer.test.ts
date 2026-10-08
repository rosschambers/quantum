import { describe, it, expect, beforeAll, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import JsonFoldRenderer from './JsonFoldRenderer.svelte';
import rendererSource from './JsonFoldRenderer.svelte?raw';
import * as highlighterModule from './highlighter';

beforeAll(() => {
    if (typeof Element.prototype.scrollIntoView !== 'function') {
        Element.prototype.scrollIntoView = function (): void {};
    }
});

const NESTED_JSON = '{"a":{"b":"needle"},"c":1}';

describe('JsonFoldRenderer search highlighting', () => {
    it('shares one vertical and horizontal scroll container with a horizontally sticky gutter', () => {
        const { container } = render(JsonFoldRenderer, { props: { content: NESTED_JSON } });
        const scroller = container.querySelector('.json-fold-renderer')!;
        const sharedLines = scroller.querySelector('.json-lines');
        expect(sharedLines).not.toBeNull();
        const gutter = scroller.querySelector('.gutter')!;
        const content = scroller.querySelector('.code-content')!;
        expect(gutter.parentElement).toBe(sharedLines);
        expect(content.parentElement).toBe(sharedLines);

        // jsdom has no layout engine; load the component's stylesheet to verify
        // the scrolling contract separately from the actual DOM structure.
        const stylesheet = document.createElement('style');
        stylesheet.textContent = rendererSource.split('<style>')[1].split('</style>')[0];
        document.head.appendChild(stylesheet);
        try {
            expect(getComputedStyle(scroller).overflow).toBe('auto');
            expect(getComputedStyle(content).overflow).toBe('visible');
            expect(getComputedStyle(gutter).position).toBe('sticky');
            expect(getComputedStyle(gutter).left).toBe('0px');
            expect(getComputedStyle(gutter).top).not.toBe('0px');
            expect(getComputedStyle(sharedLines!).minWidth).toBe('100%');
        } finally {
            stylesheet.remove();
        }
    });

    it('reveals source line 274 in the shared scroller and keeps gutter source numbers after folding', async () => {
        const content = JSON.stringify({ rows: Array.from({ length: 120 }, (_, index) => ({ value: index === 90 ? 'needle' : `row ${index}` })) });
        const { container, rerender } = render(JsonFoldRenderer, { props: { content } });
        const scroller = container.querySelector('.json-fold-renderer') as HTMLDivElement;
        const sharedLines = scroller.querySelector('.json-lines');
        expect(sharedLines).not.toBeNull();
        const firstObjectGutter = Array.from(scroller.querySelectorAll('.gutter-line')).find((line) => line.querySelector('.line-number')?.textContent === '3')!;
        await fireEvent.click(firstObjectGutter.querySelector('button')!);
        expect(scroller.querySelector('.json-line[data-line="4"]')).toBeNull();
        const sourceNumbers = Array.from(scroller.querySelectorAll('.json-line')).map((line) => line.getAttribute('data-line'));
        expect(Array.from(scroller.querySelectorAll('.gutter .line-number')).map((line) => line.textContent)).toEqual(sourceNumbers);
        expect(sourceNumbers.slice(0, 4)).toEqual(['1', '2', '3', '6']);
        const reveal = vi.spyOn(Element.prototype, 'scrollIntoView').mockImplementation(function (this: Element): void {
            expect(this.getAttribute('data-line')).toBe('274');
            expect(this.closest('.json-lines')).toBe(sharedLines);
            expect(sharedLines?.querySelector('.gutter')).not.toBeNull();
            // Simulate the browser's vertical reveal without horizontal movement.
            scroller.scrollTop = 5340;
        });
        try {
            scroller.scrollLeft = 75;
            await rerender({ query: 'needle', currentMatchIndex: 0 });
            await vi.waitFor(() => expect(reveal).toHaveBeenCalled());
            expect(scroller.scrollTop).toBe(5340);
            expect(scroller.scrollLeft).toBe(75);
            expect(scroller.querySelector('.json-line[data-line="274"] mark')?.textContent).toBe('needle');
            expect(scroller.querySelector('.json-line[data-line="4"]')).toBeNull();
            expect(Array.from(scroller.querySelectorAll('.gutter .line-number')).map((line) => line.textContent)).toEqual(sourceNumbers);
            reveal.mockClear();
            scroller.scrollTop = 0;
            await fireEvent.scroll(scroller);
            await rerender({ navigationRevision: 1 });
            await vi.waitFor(() => expect(reveal).toHaveBeenCalledOnce());
            expect(scroller.scrollTop).toBe(5340);
            expect(scroller.scrollLeft).toBe(75);
        } finally {
            reveal.mockRestore();
        }
    });

    it('highlights a matched substring with a search-match mark', () => {
        const { container } = render(JsonFoldRenderer, {
            props: { content: NESTED_JSON, query: 'needle' },
        });
        expect(container.querySelector('mark.search-match')?.textContent).toBe('needle');
    });

    it('does not render any mark when the query is empty', () => {
        const { container } = render(JsonFoldRenderer, {
            props: { content: NESTED_JSON, query: '' },
        });
        expect(container.querySelector('mark.search-match')).toBeNull();
    });

    it('reports the match count via onMatchCount', async () => {
        const counts: number[] = [];
        render(JsonFoldRenderer, {
            props: {
                content: '{"a":"needle","b":"needle"}',
                query: 'needle',
                onMatchCount: (count: number) => counts.push(count),
            },
        });
        await vi.waitFor(() => expect(counts.at(-1)).toBe(2));
    });

    it('marks the current match with an additional class', () => {
        const { container } = render(JsonFoldRenderer, {
            props: { content: '{"a":"needle","b":"needle"}', query: 'needle', currentMatchIndex: 1 },
        });
        const marks = container.querySelectorAll('mark.search-match');
        expect(marks).toHaveLength(2);
        expect(marks[1].classList.contains('search-match-current')).toBe(true);
        expect(marks[0].classList.contains('search-match-current')).toBe(false);
    });

    it('finds matches against the PRETTY-PRINTED (rendered) lines, not the raw minified source', () => {
        // The raw content is a single minified line; the rendered gutter
        // numbers lines of the pretty-printed expansion. A match that is
        // only reachable via the pretty expansion (for example appearing on
        // a specific rendered line number) proves search operates on what is
        // actually displayed, not on `content` taken literally.
        const { container } = render(JsonFoldRenderer, {
            props: { content: NESTED_JSON, query: 'needle', currentMatchIndex: 0 },
        });
        const mark = container.querySelector('mark.search-match');
        expect(mark).not.toBeNull();
        // The match's containing gutter line number must be 3 (the pretty
        // expansion's `    "b": "needle"` line), never 1 (the raw content
        // has only one line).
        const line = mark?.closest('[data-line]');
        expect(line?.getAttribute('data-line')).toBe('3');
    });

    it('expands ALL folds enclosing the current match, leaving unrelated siblings collapsed', async () => {
        const content = '{"a":{"b":{"value":"needle"}},"sibling":{"value":"other"}}';
        const { container, rerender } = render(JsonFoldRenderer, {
            props: { content },
        });
        for (const lineNumber of ['3', '7', '2', '1']) {
            const gutterLine = Array.from(container.querySelectorAll('.gutter-line')).find((line) => line.querySelector('.line-number')?.textContent === lineNumber)!;
            await fireEvent.click(gutterLine.querySelector('button')!);
        }
        expect(container.querySelector('[data-line="4"]')).toBeNull();
        expect(container.querySelector('[data-line="8"]')).toBeNull();
        await rerender({ query: 'needle', currentMatchIndex: 0 });
        expect(container.querySelector('[data-line="4"] mark')?.textContent).toBe('needle');
        expect(container.querySelector('[data-line="8"]')).toBeNull();
        expect(container.querySelectorAll('.fold-marker.collapsed')).toHaveLength(1);
        await rerender({ query: '', currentMatchIndex: null });
        expect(container.querySelector('[data-line="4"]')).not.toBeNull();
        expect(container.querySelector('[data-line="8"]')).toBeNull();
    });

    it('reuses cached base highlighting across keystrokes instead of re-highlighting every line', async () => {
        const items = Array.from({ length: 400 }, (_, index) => (index === 50 || index === 150 ? `needle${index}` : `row ${index}`));
        const content = JSON.stringify({ items });
        const spy = vi.spyOn(highlighterModule, 'highlightCode');
        const { rerender } = render(JsonFoldRenderer, {
            props: { content, query: '' },
        });
        spy.mockClear();
        await rerender({ query: 'needle' });
        expect(spy.mock.calls.length).toBeLessThan(20);
        spy.mockRestore();
    });
});
