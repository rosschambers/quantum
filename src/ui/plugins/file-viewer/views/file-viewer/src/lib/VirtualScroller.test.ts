import { describe, it, expect, beforeAll, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import Harness from './VirtualScrollerHarness.test.svelte';

// jsdom implements no layout engine and does not define ResizeObserver at
// all; VirtualScroller's mount effect constructs one unconditionally to
// track container resizes in a real browser. A minimal inert stub is enough
// for this test's purposes (it only asserts scrollTop, never a resize-driven
// recomputation).
beforeAll(() => {
    if (typeof (globalThis as any).ResizeObserver === 'undefined') {
        (globalThis as any).ResizeObserver = class {
            observe(): void {}
            unobserve(): void {}
            disconnect(): void {}
        };
    }
});

function makeLines(count: number): string[] {
    return Array.from({ length: count }, (_, i) => `line ${i}`);
}

describe('VirtualScroller scrollToIndex', () => {
    it('includes content padding in the spacer and bottom clamp with a real-sized viewport', () => {
        const height = vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(518);
        try {
            const { container } = render(Harness, { props: { lines: makeLines(1000), lineHeight: 21, verticalPadding: 32, scrollToIndex: 999 } });
            const scroller = container.querySelector('.virtual-scroller') as HTMLElement;
            expect((scroller.firstElementChild as HTMLElement).style.height).toBe('21064px');
            expect(scroller.scrollTop).toBe(21064 - 518);
            const lastRowTop = 32 + 999 * 21 - scroller.scrollTop;
            expect(lastRowTop).toBeGreaterThanOrEqual(0);
            expect(lastRowTop + 21).toBeLessThanOrEqual(518);
        } finally {
            height.mockRestore();
        }
    });
    it('reapplies a same-index request after the user scrolls away', async () => {
        const lines = makeLines(1000);
        const { container, rerender } = render(Harness, { props: { lines, lineHeight: 20, scrollToIndex: 600, scrollRequest: 1 } });
        const scroller = container.querySelector('.virtual-scroller') as HTMLDivElement;
        scroller.scrollTop = 0;
        await fireEvent.scroll(scroller);
        await rerender({ scrollRequest: 2 });
        expect(scroller.scrollTop).toBe(12000);
    });
    it('does not scroll when scrollToIndex is not provided', () => {
        const { container } = render(Harness, {
            props: { lines: makeLines(1000), lineHeight: 20, bufferLines: 5 },
        });
        const scroller = container.querySelector('.virtual-scroller') as HTMLDivElement;
        expect(scroller.scrollTop).toBe(0);
    });

    it('sets the container scrollTop to index * lineHeight when scrollToIndex changes', async () => {
        const { container, rerender } = render(Harness, {
            props: { lines: makeLines(1000), lineHeight: 20, bufferLines: 5 },
        });
        const scroller = container.querySelector('.virtual-scroller') as HTMLDivElement;

        await rerender({ lines: makeLines(1000), lineHeight: 20, bufferLines: 5, scrollToIndex: 600 });

        expect(scroller.scrollTop).toBe(12000);
    });

    it('clamps scrollToIndex to the maximum scrollable offset', async () => {
        const { container, rerender } = render(Harness, {
            props: { lines: makeLines(10), lineHeight: 20, bufferLines: 5 },
        });
        const scroller = container.querySelector('.virtual-scroller') as HTMLDivElement;

        await rerender({ lines: makeLines(10), lineHeight: 20, bufferLines: 5, scrollToIndex: 9999 });

        // jsdom performs no layout, so containerHeight (clientHeight) is 0 and
        // the maximum scroll offset is exactly lines.length * lineHeight.
        expect(scroller.scrollTop).toBe(200);
    });

    it('clamps a negative scrollToIndex to zero', async () => {
        const { container, rerender } = render(Harness, {
            props: { lines: makeLines(10), lineHeight: 20, bufferLines: 5 },
        });
        const scroller = container.querySelector('.virtual-scroller') as HTMLDivElement;

        await rerender({ lines: makeLines(10), lineHeight: 20, bufferLines: 5, scrollToIndex: -5 });

        expect(scroller.scrollTop).toBe(0);
    });

    it('existing user-scroll behavior (the scroll listener) is unaffected', () => {
        const { container } = render(Harness, {
            props: { lines: makeLines(1000), lineHeight: 20, bufferLines: 5 },
        });
        const scroller = container.querySelector('.virtual-scroller') as HTMLDivElement;
        scroller.scrollTop = 500;
        scroller.dispatchEvent(new Event('scroll'));
        expect(scroller.scrollTop).toBe(500);
    });
});
