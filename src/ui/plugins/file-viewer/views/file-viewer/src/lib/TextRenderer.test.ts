import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest';
import { render, cleanup } from '@testing-library/svelte/svelte5';
import TextRenderer from './TextRenderer.svelte';
import CodeRenderer from './CodeRenderer.svelte';
import rendererSource from './TextRenderer.svelte?raw';
import { rowCenterFraction } from './overviewRuler';

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

afterEach(() => {
    cleanup();
});

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

    it('measures wrapped match positions from live layout via requestAnimationFrame, keyed by each match line\'s span', async () => {
        const frameCallbacks: FrameRequestCallback[] = [];
        let nextFrameId = 1;
        vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
            frameCallbacks.push(callback);
            return nextFrameId++;
        });
        vi.stubGlobal('cancelAnimationFrame', () => {});
        const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
            if (this.classList.contains('text-content')) return rect({ top: 0, height: 1000 });
            if (this.getAttribute?.('data-line') === '2') return rect({ top: 400, height: 20 });
            return rect({ top: 0, height: 0 });
        });
        try {
            const positionCalls: Float64Array[] = [];
            render(TextRenderer, {
                props: { content: 'line one\nneedle here', query: 'needle', onMatchPositions: (positions: Float64Array) => positionCalls.push(positions) },
            });
            expect(frameCallbacks.length).toBeGreaterThan(0);
            for (const callback of frameCallbacks) callback(0);
            expect(positionCalls.at(-1)).toEqual(new Float64Array([400 / 1000]));
        } finally {
            rectSpy.mockRestore();
            vi.unstubAllGlobals();
        }
    });

    it('re-measures wrapped match positions when the content root resizes', async () => {
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
            if (this.classList.contains('text-content')) return rect({ top: 0, height: 1000 });
            return rect({ top: anchorTop, height: 20 });
        });
        try {
            const positionCalls: Float64Array[] = [];
            render(TextRenderer, {
                props: { content: 'needle', query: 'needle', onMatchPositions: (positions: Float64Array) => positionCalls.push(positions) },
            });
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

    it('never calls onMatchPositions when only currentMatchIndex changes (wrapped, non-virtual)', async () => {
        vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
            callback(0);
            return 1;
        });
        vi.stubGlobal('cancelAnimationFrame', () => {});
        const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
            if (this.classList.contains('text-content')) return rect({ top: 0, height: 1000 });
            return rect({ top: 100, height: 20 });
        });
        try {
            const positionCalls: Float64Array[] = [];
            const onMatchPositions = (positions: Float64Array) => positionCalls.push(positions);
            const { rerender } = render(TextRenderer, {
                props: { content: 'cat cat cat', query: 'cat', currentMatchIndex: 0, onMatchPositions },
            });
            expect(positionCalls.length).toBeGreaterThan(0);
            positionCalls.length = 0;
            await rerender({ currentMatchIndex: 1 });
            expect(positionCalls).toHaveLength(0);
        } finally {
            rectSpy.mockRestore();
            vi.unstubAllGlobals();
        }
    });

    it('wraps the content in a scrolling container, for both the plain and the search-active branch', () => {
        const stylesheet = document.createElement('style');
        stylesheet.textContent = rendererSource.split('<style>')[1].split('</style>')[0];
        document.head.appendChild(stylesheet);
        try {
            for (const query of ['', 'needle']) {
                const { container } = render(TextRenderer, {
                    props: { content: 'find the needle here', query },
                });
                const scroller = container.querySelector('.text-scroller');
                expect(scroller).not.toBeNull();
                const pre = scroller?.querySelector('pre.text-content');
                expect(pre).not.toBeNull();
                expect(getComputedStyle(scroller!).overflow).toBe('auto');
                expect(getComputedStyle(scroller!).height).toBe('100%');
            }
        } finally {
            stylesheet.remove();
        }
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

    it('reports match positions using fixed virtual row arithmetic (paddingTop/Bottom 32, rowHeight 21)', async () => {
        const lines = Array.from({ length: 600 }, (_, i) => (i === 550 ? 'needle' : `line ${i}`));
        const positionCalls: Float64Array[] = [];
        render(TextRenderer, {
            props: { content: lines.join('\n'), query: 'needle', onMatchPositions: (positions: Float64Array) => positionCalls.push(positions) },
        });
        await vi.waitFor(() => expect(positionCalls.length).toBeGreaterThan(0));
        expect(positionCalls.at(-1)).toEqual(new Float64Array([rowCenterFraction(550, 600, { paddingTop: 32, rowHeight: 21, paddingBottom: 32 })]));
    });
});
