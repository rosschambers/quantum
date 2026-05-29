import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { onScroll, onClick } from './interaction';

describe('onScroll', () => {
    let node: HTMLElement;

    beforeEach(() => {
        node = document.createElement('div');
        document.body.appendChild(node);
    });

    afterEach(() => {
        document.body.removeChild(node);
    });

    it('fires handler with +1 on upward scroll', () => {
        const handler = vi.fn();
        const teardown = onScroll(node, handler);
        node.dispatchEvent(new WheelEvent('wheel', { deltaY: -10 }));
        expect(handler).toHaveBeenCalledTimes(1);
        expect(handler).toHaveBeenCalledWith(1);
        teardown();
    });

    it('fires handler with -1 on downward scroll', () => {
        const handler = vi.fn();
        const teardown = onScroll(node, handler);
        node.dispatchEvent(new WheelEvent('wheel', { deltaY: 10 }));
        expect(handler).toHaveBeenCalledWith(-1);
        teardown();
    });

    it('debounces rapid scroll events to one per 50ms', () => {
        vi.useFakeTimers();
        vi.setSystemTime(new Date(2026, 0, 1, 0, 0, 0));
        const handler = vi.fn();
        const teardown = onScroll(node, handler);

        node.dispatchEvent(new WheelEvent('wheel', { deltaY: -10 }));
        node.dispatchEvent(new WheelEvent('wheel', { deltaY: -10 }));
        node.dispatchEvent(new WheelEvent('wheel', { deltaY: -10 }));

        expect(handler).toHaveBeenCalledTimes(1);

        vi.setSystemTime(new Date(2026, 0, 1, 0, 0, 0, 60));
        node.dispatchEvent(new WheelEvent('wheel', { deltaY: -10 }));

        expect(handler).toHaveBeenCalledTimes(2);

        teardown();
        vi.useRealTimers();
    });

    it('teardown removes the listener', () => {
        const handler = vi.fn();
        const teardown = onScroll(node, handler);
        teardown();
        node.dispatchEvent(new WheelEvent('wheel', { deltaY: -10 }));
        expect(handler).not.toHaveBeenCalled();
    });
});

describe('onClick', () => {
    let node: HTMLElement;

    beforeEach(() => {
        node = document.createElement('div');
        document.body.appendChild(node);
    });

    afterEach(() => {
        document.body.removeChild(node);
    });

    it('fires handler on left click by default', () => {
        const handler = vi.fn();
        const teardown = onClick(node, handler);
        node.dispatchEvent(new MouseEvent('click'));
        expect(handler).toHaveBeenCalledTimes(1);
        teardown();
    });

    it('does not fire left handler on contextmenu', () => {
        const handler = vi.fn();
        const teardown = onClick(node, handler, 'left');
        node.dispatchEvent(new MouseEvent('contextmenu'));
        expect(handler).not.toHaveBeenCalled();
        teardown();
    });

    it('fires handler on right click when button=right', () => {
        const handler = vi.fn();
        const teardown = onClick(node, handler, 'right');
        node.dispatchEvent(new MouseEvent('contextmenu'));
        expect(handler).toHaveBeenCalledTimes(1);
        teardown();
    });

    it('does not fire right handler on plain click', () => {
        const handler = vi.fn();
        const teardown = onClick(node, handler, 'right');
        node.dispatchEvent(new MouseEvent('click'));
        expect(handler).not.toHaveBeenCalled();
        teardown();
    });

    it('teardown removes the listener', () => {
        const handler = vi.fn();
        const teardown = onClick(node, handler);
        teardown();
        node.dispatchEvent(new MouseEvent('click'));
        expect(handler).not.toHaveBeenCalled();
    });
});
