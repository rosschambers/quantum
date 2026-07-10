import { describe, it, expect, beforeEach } from 'vitest';
import { setClipboard, getClipboard, clearClipboard } from './clipboard.svelte';

describe('clipboard', () => {
    beforeEach(() => {
        clearClipboard();
    });

    it('starts empty', () => {
        expect(getClipboard()).toBeNull();
    });

    it('stores a copy operation with its paths', () => {
        setClipboard('copy', ['/home/user/a.txt', '/home/user/b.txt']);
        expect(getClipboard()).toEqual({
            operation: 'copy',
            paths: ['/home/user/a.txt', '/home/user/b.txt'],
        });
    });

    it('stores a cut operation', () => {
        setClipboard('cut', ['/home/user/a.txt']);
        expect(getClipboard()).toEqual({ operation: 'cut', paths: ['/home/user/a.txt'] });
    });

    it('replaces earlier contents on a subsequent set', () => {
        setClipboard('copy', ['/home/user/a.txt']);
        setClipboard('cut', ['/home/user/b.txt']);
        expect(getClipboard()).toEqual({ operation: 'cut', paths: ['/home/user/b.txt'] });
    });

    it('clears back to null', () => {
        setClipboard('copy', ['/home/user/a.txt']);
        clearClipboard();
        expect(getClipboard()).toBeNull();
    });
});
