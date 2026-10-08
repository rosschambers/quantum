import { describe, test, expect, vi, afterEach } from 'vitest';
import { measureFractions, observeLayout } from './measureOffsets';

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

describe('measureFractions', () => {
    afterEach(() => {
        vi.restoreAllMocks();
    });

    test('reads the root rect exactly once regardless of anchor count', () => {
        const root = document.createElement('div');
        const spy = vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(rect({ top: 100, height: 500 }));
        const anchors = [document.createElement('span'), document.createElement('span'), document.createElement('span')];
        for (const anchor of anchors) {
            vi.spyOn(anchor, 'getBoundingClientRect').mockReturnValue(rect({ top: 150, height: 10 }));
        }
        measureFractions(root, anchors);
        expect(spy).toHaveBeenCalledTimes(1);
    });

    test('returns the fraction of each anchor top relative to the root', () => {
        const root = document.createElement('div');
        vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(rect({ top: 100, height: 400 }));
        const anchor = document.createElement('span');
        vi.spyOn(anchor, 'getBoundingClientRect').mockReturnValue(rect({ top: 300, height: 10 }));
        const result = measureFractions(root, [anchor]);
        expect(result[0]).toBeCloseTo((300 - 100) / 400, 10);
    });

    test('falls back to the nearest ancestor with a non-zero rect for a zero-size anchor', () => {
        const root = document.createElement('div');
        vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(rect({ top: 0, height: 1000 }));
        const parent = document.createElement('div');
        const anchor = document.createElement('span');
        parent.appendChild(anchor);
        root.appendChild(parent);
        vi.spyOn(anchor, 'getBoundingClientRect').mockReturnValue(rect({ top: 0, height: 0, width: 0 }));
        vi.spyOn(parent, 'getBoundingClientRect').mockReturnValue(rect({ top: 250, height: 20 }));
        const result = measureFractions(root, [anchor]);
        expect(result[0]).toBeCloseTo(250 / 1000, 10);
    });

    test('falls back to the ancestor of a Range anchor\'s start container when the range itself has zero size', () => {
        const root = document.createElement('div');
        vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(rect({ top: 0, height: 1000 }));
        const parent = document.createElement('span');
        const textNode = document.createTextNode('needle');
        parent.appendChild(textNode);
        root.appendChild(parent);
        vi.spyOn(parent, 'getBoundingClientRect').mockReturnValue(rect({ top: 400, height: 20 }));

        const range = document.createRange();
        range.setStart(textNode, 0);
        range.setEnd(textNode, 0);
        // jsdom's Range has no `getBoundingClientRect` of its own to spy on
        // (no layout engine), so it is assigned directly, as a real collapsed
        // range's rect would be: zero width and height.
        (range as unknown as { getBoundingClientRect: () => DOMRect }).getBoundingClientRect = () => rect({ top: 0, height: 0, width: 0 });

        const result = measureFractions(root, [range]);
        expect(result[0]).toBeCloseTo(400 / 1000, 10);
    });

    test('a collapsed Range whose start container is itself an Element (not Text) falls back directly to it', () => {
        const root = document.createElement('div');
        vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(rect({ top: 0, height: 1000 }));
        const container = document.createElement('div');
        root.appendChild(container);
        vi.spyOn(container, 'getBoundingClientRect').mockReturnValue(rect({ top: 250, height: 30 }));

        const range = document.createRange();
        range.setStart(container, 0);
        range.setEnd(container, 0);
        (range as unknown as { getBoundingClientRect: () => DOMRect }).getBoundingClientRect = () => rect({ top: 0, height: 0, width: 0 });

        const result = measureFractions(root, [range]);
        expect(result[0]).toBeCloseTo(250 / 1000, 10);
    });

    test('returns an empty array when the root has zero height, with no NaN', () => {
        const root = document.createElement('div');
        vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(rect({ top: 0, height: 0 }));
        const anchor = document.createElement('span');
        const result = measureFractions(root, [anchor]);
        expect(result).toHaveLength(0);
        expect(Array.from(result).some((value) => Number.isNaN(value))).toBe(false);
    });

    test('performs no DOM mutations', () => {
        const root = document.createElement('div');
        document.body.appendChild(root);
        vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(rect({ top: 0, height: 100 }));
        const anchor = document.createElement('span');
        root.appendChild(anchor);
        vi.spyOn(anchor, 'getBoundingClientRect').mockReturnValue(rect({ top: 10, height: 10 }));

        const observer = new MutationObserver(() => {});
        observer.observe(root, { attributes: true, childList: true, subtree: true, characterData: true });

        measureFractions(root, [anchor]);

        expect(observer.takeRecords()).toEqual([]);
        observer.disconnect();
        root.remove();
    });
});

describe('observeLayout', () => {
    afterEach(() => {
        vi.unstubAllGlobals();
    });

    test('coalesces several resize-observer callbacks into one onChange per animation frame', () => {
        const frameCallbacks: FrameRequestCallback[] = [];
        let nextFrameId = 1;
        vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
            frameCallbacks.push(callback);
            return nextFrameId++;
        });
        vi.stubGlobal('cancelAnimationFrame', () => {});

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

        const onChange = vi.fn();
        const root = document.createElement('div');
        const stop = observeLayout(root, onChange);

        resizeCallback?.([], {} as ResizeObserver);
        resizeCallback?.([], {} as ResizeObserver);
        resizeCallback?.([], {} as ResizeObserver);

        expect(frameCallbacks).toHaveLength(1);
        frameCallbacks[0](0);
        expect(onChange).toHaveBeenCalledTimes(1);

        stop();
    });

    test('the returned stop function disconnects the observer and cancels a pending frame', () => {
        const cancelledIds: number[] = [];
        let nextFrameId = 1;
        vi.stubGlobal('requestAnimationFrame', () => nextFrameId++);
        vi.stubGlobal('cancelAnimationFrame', (id: number) => cancelledIds.push(id));

        let resizeCallback: ResizeObserverCallback | undefined;
        let disconnected = false;
        vi.stubGlobal(
            'ResizeObserver',
            class {
                constructor(callback: ResizeObserverCallback) {
                    resizeCallback = callback;
                }
                observe(): void {}
                unobserve(): void {}
                disconnect(): void {
                    disconnected = true;
                }
            },
        );

        const root = document.createElement('div');
        const stop = observeLayout(root, () => {});
        resizeCallback?.([], {} as ResizeObserver);

        stop();

        expect(disconnected).toBe(true);
        expect(cancelledIds).toEqual([1]);
    });
});
