import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import CodeRenderer from './CodeRenderer.svelte';
import rendererSource from './CodeRenderer.svelte?raw';
import * as highlighterModule from './highlighter';
import { rowCenterFraction } from './overviewRuler';

beforeAll(() => {
    if (typeof (globalThis as any).ResizeObserver === 'undefined') {
        (globalThis as any).ResizeObserver = class {
            observe(): void {}
            unobserve(): void {}
            disconnect(): void {}
        };
    }
    // jsdom defines no layout engine and does not implement scrollIntoView at
    // all (it is simply absent from the Element prototype).
    if (typeof Element.prototype.scrollIntoView !== 'function') {
        Element.prototype.scrollIntoView = function (): void {};
    }
});

afterEach(() => {
    cleanup();
});

describe('CodeRenderer search highlighting (non-virtualized)', () => {
    it('highlights a matched substring with a search-match mark', () => {
        const { container } = render(CodeRenderer, {
            props: { content: 'const needle = 1;', language: 'javascript', query: 'needle' },
        });
        expect(container.querySelector('mark.search-match')?.textContent).toBe('needle');
    });

    it('does not render any mark when the query is empty', () => {
        const { container } = render(CodeRenderer, {
            props: { content: 'const needle = 1;', language: 'javascript', query: '' },
        });
        expect(container.querySelector('mark.search-match')).toBeNull();
    });

    it('reports the match count via onMatchCount', async () => {
        const counts: number[] = [];
        render(CodeRenderer, {
            props: {
                content: 'cat cat cat',
                query: 'cat',
                onMatchCount: (count: number) => counts.push(count),
            },
        });
        await vi.waitFor(() => expect(counts.at(-1)).toBe(3));
    });

    it('marks the current match with an additional class', () => {
        const { container } = render(CodeRenderer, {
            props: { content: 'cat cat cat', query: 'cat', currentMatchIndex: 1 },
        });
        const marks = container.querySelectorAll('mark.search-match');
        expect(marks).toHaveLength(3);
        expect(marks[1].classList.contains('search-match-current')).toBe(true);
        expect(marks[0].classList.contains('search-match-current')).toBe(false);
        expect(marks[2].classList.contains('search-match-current')).toBe(false);
    });

    it('expands ALL folds enclosing the current match, leaving unrelated folds collapsed', async () => {
        const lines = [
            'function outer() {',
            '  const a = 1;',
            '  function inner() {',
            '    return needle;',
            '  }',
            '  return a;',
            '}',
            'function unrelated() {',
            '  return 2;',
            '}',
        ];
        const content = lines.join('\n');
        const { container, rerender } = render(CodeRenderer, {
            props: { content, language: 'javascript' },
        });
        for (const lineNumber of ['3', '8', '1']) {
            const gutterLine = Array.from(container.querySelectorAll('.gutter-line')).find((line) => line.querySelector('.line-number')?.textContent === lineNumber)!;
            await fireEvent.click(gutterLine.querySelector('button')!);
        }
        expect(container.querySelector('[data-line="4"]')).toBeNull();
        expect(container.querySelector('[data-line="9"]')).toBeNull();
        await rerender({ query: 'needle', currentMatchIndex: 0 });
        expect(container.querySelector('[data-line="4"] mark')?.textContent).toBe('needle');
        expect(container.querySelector('[data-line="9"]')).toBeNull();
        expect(container.querySelectorAll('.fold-marker.collapsed')).toHaveLength(1);
        await rerender({ query: '', currentMatchIndex: null });
        expect(container.querySelector('[data-line="4"]')).not.toBeNull();
        expect(container.querySelector('[data-line="9"]')).toBeNull();
    });

    it('reuses cached base highlighting across keystrokes instead of re-highlighting every line', async () => {
        const lines = Array.from({ length: 400 }, (_, index) => (index === 50 || index === 150 ? `const needle${index} = 1;` : `const line${index} = ${index};`));
        const content = lines.join('\n');
        const spy = vi.spyOn(highlighterModule, 'highlightCode');
        const { rerender } = render(CodeRenderer, {
            props: { content, language: 'javascript', query: '' },
        });
        spy.mockClear();
        await rerender({ query: 'needle' });
        expect(spy.mock.calls.length).toBeLessThan(20);
        spy.mockRestore();
    });

    it('reports match positions mapped through visible rows, remapping a collapsed fold to its header row and splitting after expanding', async () => {
        const lines = [
            'function outer() {',
            '  const a = 1;',
            '  function inner() {',
            '    return needle;',
            '  }',
            '  return a;',
            '}',
        ];
        const content = lines.join('\n');
        const positionCalls: Float64Array[] = [];
        const onMatchPositions = (positions: Float64Array) => positionCalls.push(positions);
        const { container, rerender } = render(CodeRenderer, {
            props: { content, language: 'javascript', onMatchPositions },
        });
        const gutterLine = Array.from(container.querySelectorAll('.gutter-line')).find((line) => line.querySelector('.line-number')?.textContent === '3')!;
        await fireEvent.click(gutterLine.querySelector('button')!);
        positionCalls.length = 0;
        await rerender({ content, language: 'javascript', query: 'needle', onMatchPositions });
        await vi.waitFor(() => expect(positionCalls.length).toBeGreaterThan(0));
        // Indentation-based folding at line index 2 ("function inner() {")
        // only covers index 3 (the deeper-indented body); the closing brace
        // at index 4 sits back at the same indent and is never swallowed —
        // see the identical boundary already exercised by this file's
        // "expands ALL folds enclosing the current match" test. Collapsed,
        // line index 3 (needle) therefore maps to row 2 (the header's row),
        // out of a row count of 6: 0, 1, 2-header(=3 hidden), 4, 5, 6.
        expect(positionCalls.at(-1)).toEqual(new Float64Array([rowCenterFraction(2, 6, { paddingTop: 12, rowHeight: 20.8, paddingBottom: 12 })]));

        positionCalls.length = 0;
        await fireEvent.click(gutterLine.querySelector('button')!);
        await vi.waitFor(() => expect(positionCalls.length).toBeGreaterThan(0));
        // Expanded: line index 3 now maps to its own row 3, out of 7 rows.
        expect(positionCalls.at(-1)).toEqual(new Float64Array([rowCenterFraction(3, 7, { paddingTop: 12, rowHeight: 20.8, paddingBottom: 12 })]));
    });

    it('never calls onMatchPositions when only currentMatchIndex changes', async () => {
        const positionCalls: Float64Array[] = [];
        const onMatchPositions = (positions: Float64Array) => positionCalls.push(positions);
        const { rerender } = render(CodeRenderer, {
            props: { content: 'cat cat cat', query: 'cat', currentMatchIndex: 0, onMatchPositions },
        });
        await vi.waitFor(() => expect(positionCalls.length).toBeGreaterThan(0));
        positionCalls.length = 0;
        await rerender({ currentMatchIndex: 1 });
        expect(positionCalls).toHaveLength(0);
    });

    it('scrolls the gutter together with the code in one shared scroll container', () => {
        const stylesheet = document.createElement('style');
        stylesheet.textContent = rendererSource.split('<style>')[1].split('</style>')[0];
        document.head.appendChild(stylesheet);
        try {
            const { container } = render(CodeRenderer, {
                props: { content: 'const needle = 1;\nconst other = 2;', language: 'javascript' },
            });
            const scroller = container.querySelector('.code-scroller');
            expect(scroller).not.toBeNull();
            const gutter = container.querySelector('.gutter')!;
            const code = container.querySelector('.code-content')!;
            expect(gutter.closest('.code-scroller')).toBe(scroller);
            expect(code.closest('.code-scroller')).toBe(scroller);
            expect(getComputedStyle(scroller!).overflow).toBe('auto');
            expect(getComputedStyle(scroller!).height).toBe('100%');
            expect(getComputedStyle(gutter).position).toBe('sticky');
            expect(getComputedStyle(gutter).left).toBe('0px');
        } finally {
            stylesheet.remove();
        }
    });
});

describe('CodeRenderer search highlighting (virtualized, >500 lines)', () => {
    it('numbers the mounted lines from their original offset and scrolls gutter with content', async () => {
        const content = Array.from({ length: 1000 }, (_, index) => `line ${index + 1}`).join('\n');
        const { container } = render(CodeRenderer, { props: { content } });
        const scroller = container.querySelector('.virtual-scroller') as HTMLDivElement;
        expect(scroller.style.flexGrow).toBe('1');
        expect(scroller.style.minWidth).toBe('0px');
        scroller.scrollTop = 900 * 21;
        await fireEvent.scroll(scroller);
        const gutter = scroller.querySelector('.line-numbers')!;
        expect(gutter).not.toBeNull();
        expect(gutter.querySelector('.line-number')?.textContent).toBe('851');
        expect(gutter.parentElement).toBe(scroller.querySelector('.code-content')?.parentElement);
        expect(scroller.querySelector('.code-content')?.textContent).toMatch(/^line 851\n/);
        scroller.scrollTop = 200 * 21;
        await fireEvent.scroll(scroller);
        expect(gutter.querySelector('.line-number')?.textContent).toBe('151');
    });
    it('reveals an offscreen match by scrolling the virtual window to include it', async () => {
        const lines = Array.from({ length: 600 }, (_, i) => (i === 550 ? 'const needle = 1;' : `const line${i} = ${i};`));
        const { container } = render(CodeRenderer, {
            props: { content: lines.join('\n'), language: 'javascript', query: 'needle', currentMatchIndex: 0 },
        });

        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')).not.toBeNull();
        });
        expect(container.querySelector('mark.search-match')?.textContent).toBe('needle');
    });

    it('reports match positions using fixed virtual row arithmetic (paddingTop/Bottom 12, rowHeight 21)', async () => {
        const lines = Array.from({ length: 600 }, (_, i) => (i === 550 ? 'const needle = 1;' : `const line${i} = ${i};`));
        const positionCalls: Float64Array[] = [];
        render(CodeRenderer, {
            props: {
                content: lines.join('\n'),
                language: 'javascript',
                query: 'needle',
                onMatchPositions: (positions: Float64Array) => positionCalls.push(positions),
            },
        });
        await vi.waitFor(() => expect(positionCalls.length).toBeGreaterThan(0));
        expect(positionCalls.at(-1)).toEqual(new Float64Array([rowCenterFraction(550, 600, { paddingTop: 12, rowHeight: 21, paddingBottom: 12 })]));
    });

});
