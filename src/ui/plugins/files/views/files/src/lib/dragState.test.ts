import { describe, it, expect, afterEach } from 'vitest';
import { beginDrag, endDrag, getDragSources } from './dragState.svelte';

afterEach(() => {
    endDrag();
});

describe('dragState', () => {
    it('has no sources before a drag begins', () => {
        expect(getDragSources()).toBeNull();
    });

    it('records the sources handed to beginDrag', () => {
        beginDrag(['/a', '/b']);
        expect(getDragSources()).toEqual(['/a', '/b']);
    });

    it('clears the sources on endDrag', () => {
        beginDrag(['/a']);
        expect(getDragSources()).toEqual(['/a']);
        endDrag();
        expect(getDragSources()).toBeNull();
    });

    it('replaces earlier sources when beginDrag is called again', () => {
        beginDrag(['/first']);
        beginDrag(['/second', '/third']);
        expect(getDragSources()).toEqual(['/second', '/third']);
    });
});
