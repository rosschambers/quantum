import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
    toasts,
    pushToast,
    dismissToast,
    TOAST_DURATION_MILLISECONDS,
} from './toasts.svelte';

describe('toasts store', () => {
    beforeEach(() => {
        vi.useFakeTimers();
        toasts.splice(0, toasts.length);
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('adds an info toast by default with a unique id', () => {
        const firstId = pushToast('Copied');
        const secondId = pushToast('Moved');
        expect(toasts).toHaveLength(2);
        expect(firstId).not.toBe(secondId);
        expect(toasts[0]).toMatchObject({ message: 'Copied', kind: 'info' });
    });

    it('flags an error toast', () => {
        pushToast('Operation failed', 'error');
        expect(toasts[0].kind).toBe('error');
    });

    it('auto-removes a toast after the toast duration', () => {
        pushToast('Copied');
        expect(toasts).toHaveLength(1);
        vi.advanceTimersByTime(TOAST_DURATION_MILLISECONDS);
        expect(toasts).toHaveLength(0);
    });

    it('leaves other toasts in place when one auto-dismisses', () => {
        pushToast('First');
        vi.advanceTimersByTime(TOAST_DURATION_MILLISECONDS / 2);
        pushToast('Second');
        vi.advanceTimersByTime(TOAST_DURATION_MILLISECONDS / 2);
        expect(toasts).toHaveLength(1);
        expect(toasts[0].message).toBe('Second');
    });

    it('dismisses a toast by id on demand', () => {
        const id = pushToast('Copied');
        dismissToast(id);
        expect(toasts).toHaveLength(0);
    });
});
