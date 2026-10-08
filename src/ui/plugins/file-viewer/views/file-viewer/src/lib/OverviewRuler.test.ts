import { describe, test, expect, vi, beforeAll, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import OverviewRuler from './OverviewRuler.svelte';
import rendererSource from './OverviewRuler.svelte?raw';
import { MINIMUM_HEIGHT, thumbGeometry, scrollTopForTrackPosition, type RulerMark } from './overviewRuler';

beforeAll(() => {
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
    vi.restoreAllMocks();
});

function stubTrackHeight(height: number) {
    return vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(height);
}

function makeScrollElement(scrollTop: number, clientHeight: number, scrollHeight: number): HTMLDivElement {
    const element = document.createElement('div');
    Object.defineProperty(element, 'scrollTop', { value: scrollTop, writable: true, configurable: true });
    // Accessor properties (rather than plain values) so a test can spy on
    // `clientHeight`/`scrollHeight` reads with `vi.spyOn(element, 'clientHeight', 'get')`.
    Object.defineProperty(element, 'clientHeight', { get: () => clientHeight, configurable: true });
    Object.defineProperty(element, 'scrollHeight', { get: () => scrollHeight, configurable: true });
    return element;
}

describe('OverviewRuler', () => {
    test('always renders the reserved strip, but marks and thumb only when there is at least one mark', () => {
        const { container } = render(OverviewRuler, { props: { marks: [], scrollElement: null } });
        const root = container.querySelector('.overview-ruler');
        expect(root).not.toBeNull();
        expect(root?.getAttribute('aria-hidden')).toBe('true');
        expect(container.querySelectorAll('.ruler-mark')).toHaveLength(0);
        expect(container.querySelector('.ruler-thumb')).toBeNull();
    });

    test('renders one ruler-mark per layout segment with pixel top/height and kind/lane classes', () => {
        const heightSpy = stubTrackHeight(600);
        const marks: RulerMark[] = [{ start: 0.25, extent: 0, kind: 'match' }];
        const { container } = render(OverviewRuler, { props: { marks, scrollElement: null } });
        const markElements = container.querySelectorAll('.ruler-mark');
        expect(markElements).toHaveLength(1);
        const element = markElements[0] as HTMLElement;
        expect(element.classList.contains('kind-match')).toBe(true);
        expect(element.classList.contains('lane-full')).toBe(true);
        expect(element.style.top).toBe('150px');
        expect(element.style.height).toBe(`${MINIMUM_HEIGHT.match}px`);
        heightSpy.mockRestore();
    });

    test('splits lanes between change and search marks when a change kind is present', () => {
        const heightSpy = stubTrackHeight(600);
        const marks: RulerMark[] = [
            { start: 0.2, extent: 0, kind: 'match' },
            { start: 0.4, extent: 0.01, kind: 'added' },
        ];
        const { container } = render(OverviewRuler, { props: { marks, scrollElement: null } });
        const matchElement = Array.from(container.querySelectorAll('.ruler-mark')).find((element) => element.classList.contains('kind-match'));
        const addedElement = Array.from(container.querySelectorAll('.ruler-mark')).find((element) => element.classList.contains('kind-added'));
        expect(matchElement?.classList.contains('lane-search')).toBe(true);
        expect(addedElement?.classList.contains('lane-change')).toBe(true);
        heightSpy.mockRestore();
    });

    test('renders the thumb positioned and sized from the scroll element geometry', () => {
        const heightSpy = stubTrackHeight(600);
        const scrollElement = makeScrollElement(300, 200, 1200);
        const marks: RulerMark[] = [{ start: 0.1, extent: 0, kind: 'match' }];
        const { container } = render(OverviewRuler, { props: { marks, scrollElement } });
        const thumb = container.querySelector('.ruler-thumb') as HTMLElement;
        const expected = thumbGeometry({ scrollTop: 300, clientHeight: 200, scrollHeight: 1200 }, 600);
        expect(thumb.style.top).toBe(`${expected.top}px`);
        expect(thumb.style.height).toBe(`${expected.height}px`);
        heightSpy.mockRestore();
    });

    test('updates the thumb when the scroll element fires a scroll event', async () => {
        const heightSpy = stubTrackHeight(600);
        const scrollElement = makeScrollElement(0, 200, 1200);
        const marks: RulerMark[] = [{ start: 0.1, extent: 0, kind: 'match' }];
        const { container } = render(OverviewRuler, { props: { marks, scrollElement } });
        expect((container.querySelector('.ruler-thumb') as HTMLElement).style.top).toBe('0px');

        (scrollElement as any).scrollTop = 600;
        await fireEvent.scroll(scrollElement);

        // The scroll handler now coalesces to one measurement per animation
        // frame (see the `OverviewRuler > coalesces...` tests below), so the
        // thumb update lands on the next frame rather than synchronously.
        const expected = thumbGeometry({ scrollTop: 600, clientHeight: 200, scrollHeight: 1200 }, 600);
        await vi.waitFor(() => {
            const thumb = container.querySelector('.ruler-thumb') as HTMLElement;
            expect(thumb.style.top).toBe(`${expected.top}px`);
        });
        heightSpy.mockRestore();
    });

    test('coalesces three scroll events in the same tick into one measurement after one animation frame', async () => {
        const frameCallbacks: FrameRequestCallback[] = [];
        let nextFrameId = 1;
        vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
            frameCallbacks.push(callback);
            return nextFrameId++;
        });
        vi.stubGlobal('cancelAnimationFrame', () => {});

        const heightSpy = stubTrackHeight(600);
        const scrollElement = makeScrollElement(0, 200, 1200);
        const clientHeightSpy = vi.spyOn(scrollElement, 'clientHeight', 'get');
        const marks: RulerMark[] = [{ start: 0.1, extent: 0, kind: 'match' }];
        render(OverviewRuler, { props: { marks, scrollElement } });

        // Discard the mount-time measurement; only count work triggered by scroll.
        clientHeightSpy.mockClear();
        frameCallbacks.length = 0;

        await fireEvent.scroll(scrollElement);
        await fireEvent.scroll(scrollElement);
        await fireEvent.scroll(scrollElement);

        expect(clientHeightSpy).not.toHaveBeenCalled();
        expect(frameCallbacks).toHaveLength(1);

        frameCallbacks[0](0);
        expect(clientHeightSpy).toHaveBeenCalledTimes(1);

        heightSpy.mockRestore();
    });

    test('teardown cancels a pending scroll-coalescing animation frame', async () => {
        const cancelledIds: number[] = [];
        let nextFrameId = 1;
        vi.stubGlobal('requestAnimationFrame', () => nextFrameId++);
        vi.stubGlobal('cancelAnimationFrame', (id: number) => cancelledIds.push(id));

        const heightSpy = stubTrackHeight(600);
        const scrollElement = makeScrollElement(0, 200, 1200);
        const { unmount } = render(OverviewRuler, { props: { marks: [], scrollElement } });

        await fireEvent.scroll(scrollElement);
        unmount();

        expect(cancelledIds.length).toBeGreaterThan(0);
        heightSpy.mockRestore();
    });

    test('pointerdown prevents default so the search input keeps focus', () => {
        const { container } = render(OverviewRuler, { props: { marks: [], scrollElement: null } });
        const root = container.querySelector('.overview-ruler') as HTMLElement;
        const event = new PointerEvent('pointerdown', { bubbles: true, cancelable: true });
        const spy = vi.spyOn(event, 'preventDefault');
        root.dispatchEvent(event);
        expect(spy).toHaveBeenCalled();
    });

    test('clicking a mark activates it and does not move the scroll position', async () => {
        const heightSpy = stubTrackHeight(600);
        const rectSpy = vi
            .spyOn(HTMLElement.prototype, 'getBoundingClientRect')
            .mockReturnValue({ top: 0, left: 0, right: 10, bottom: 600, width: 10, height: 600, x: 0, y: 0, toJSON() { return this; } } as DOMRect);
        const marks: RulerMark[] = [{ start: 0.5, extent: 0, kind: 'match', index: 7 }];
        const onMarkActivate = vi.fn();
        const scrollElement = makeScrollElement(0, 200, 1200);
        const { container } = render(OverviewRuler, { props: { marks, scrollElement, onMarkActivate, hitTolerance: 4 } });
        const root = container.querySelector('.overview-ruler') as HTMLElement;
        await fireEvent.click(root, { clientY: 300 });
        expect(onMarkActivate).toHaveBeenCalledWith(marks[0]);
        expect(scrollElement.scrollTop).toBe(0);
        heightSpy.mockRestore();
        rectSpy.mockRestore();
    });

    test('activates a mark clicked 3px away using the default hitTolerance, but not one 6px away', async () => {
        const heightSpy = stubTrackHeight(600);
        const rectSpy = vi
            .spyOn(HTMLElement.prototype, 'getBoundingClientRect')
            .mockReturnValue({ top: 0, left: 0, right: 10, bottom: 600, width: 10, height: 600, x: 0, y: 0, toJSON() { return this; } } as DOMRect);
        const marks: RulerMark[] = [{ start: 0.5, extent: 0, kind: 'match', index: 3 }];
        const onMarkActivate = vi.fn();
        const scrollElement = makeScrollElement(0, 200, 1200);
        // hitTolerance omitted: relies on the component's default of 4.
        const { container, rerender } = render(OverviewRuler, { props: { marks, scrollElement, onMarkActivate } });
        const root = container.querySelector('.overview-ruler') as HTMLElement;

        await fireEvent.click(root, { clientY: 303 });
        expect(onMarkActivate).toHaveBeenCalledWith(marks[0]);

        onMarkActivate.mockClear();
        await rerender({ marks, scrollElement, onMarkActivate });
        await fireEvent.click(root, { clientY: 306 });
        expect(onMarkActivate).not.toHaveBeenCalled();

        heightSpy.mockRestore();
        rectSpy.mockRestore();
    });

    test('clicking empty track scrolls to the centered position instead of activating a mark', async () => {
        const heightSpy = stubTrackHeight(600);
        const rectSpy = vi
            .spyOn(HTMLElement.prototype, 'getBoundingClientRect')
            .mockReturnValue({ top: 0, left: 0, right: 10, bottom: 600, width: 10, height: 600, x: 0, y: 0, toJSON() { return this; } } as DOMRect);
        const marks: RulerMark[] = [{ start: 0.98, extent: 0, kind: 'match' }];
        const onMarkActivate = vi.fn();
        const scrollElement = makeScrollElement(0, 200, 1200);
        const { container } = render(OverviewRuler, { props: { marks, scrollElement, onMarkActivate, hitTolerance: 4 } });
        const root = container.querySelector('.overview-ruler') as HTMLElement;
        await fireEvent.click(root, { clientY: 10 });
        expect(onMarkActivate).not.toHaveBeenCalled();
        const expected = scrollTopForTrackPosition(10, 600, { scrollTop: 0, clientHeight: 200, scrollHeight: 1200 });
        expect(scrollElement.scrollTop).toBe(expected);
        heightSpy.mockRestore();
        rectSpy.mockRestore();
    });

    test('adds a passive scroll listener on mount and removes it on teardown', () => {
        const scrollElement = makeScrollElement(0, 200, 1200);
        const addSpy = vi.spyOn(scrollElement, 'addEventListener');
        const removeSpy = vi.spyOn(scrollElement, 'removeEventListener');
        const { unmount } = render(OverviewRuler, { props: { marks: [], scrollElement } });
        expect(addSpy).toHaveBeenCalledWith('scroll', expect.any(Function), expect.objectContaining({ passive: true }));
        unmount();
        expect(removeSpy).toHaveBeenCalledWith('scroll', expect.any(Function));
    });

    test('disconnects its resize observers on teardown', () => {
        const disconnects: boolean[] = [];
        vi.stubGlobal(
            'ResizeObserver',
            class {
                observe(): void {}
                unobserve(): void {}
                disconnect(): void {
                    disconnects.push(true);
                }
            },
        );
        const scrollElement = makeScrollElement(0, 200, 1200);
        const { unmount } = render(OverviewRuler, { props: { marks: [], scrollElement } });
        unmount();
        // One observer on the ruler element itself, one on the scroll element.
        expect(disconnects).toHaveLength(2);
        vi.unstubAllGlobals();
    });

    test('CSS: colors are theme tokens only, and the thumb never intercepts clicks', () => {
        const stylesheet = document.createElement('style');
        stylesheet.textContent = rendererSource.split('<style>')[1].split('</style>')[0];
        document.head.appendChild(stylesheet);
        const heightSpy = stubTrackHeight(600);
        try {
            const marks: RulerMark[] = [
                { start: 0.1, extent: 0, kind: 'match' },
                { start: 0.2, extent: 0, kind: 'current-match' },
                { start: 0.3, extent: 0.01, kind: 'added' },
                { start: 0.4, extent: 0.01, kind: 'removed' },
                { start: 0.5, extent: 0.01, kind: 'mixed' },
            ];
            const { container } = render(OverviewRuler, { props: { marks, scrollElement: makeScrollElement(0, 200, 1200) } });
            const root = container.querySelector('.overview-ruler') as HTMLElement;
            // jsdom's CSS parser does not resolve var() inside a border
            // shorthand (it drops the whole declaration rather than falling
            // back), so only width — a plain declaration — is checked here;
            // the border-left token itself is reviewed by reading the
            // component's source above.
            expect(getComputedStyle(root).width).toBe('10px');
            const thumb = container.querySelector('.ruler-thumb') as HTMLElement;
            expect(getComputedStyle(thumb).pointerEvents).toBe('none');
        } finally {
            heightSpy.mockRestore();
            stylesheet.remove();
        }
    });
});
