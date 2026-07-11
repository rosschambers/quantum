import { describe, it, expect } from 'vitest';
import { isValidDrop } from './dnd';

describe('isValidDrop', () => {
    it('rejects a destination that is one of the sources', () => {
        expect(isValidDrop(['/a', '/b'], '/a')).toBe(false);
        expect(isValidDrop(['/a', '/b'], '/b')).toBe(false);
    });

    it('rejects dropping a folder into itself (destination below a source)', () => {
        expect(isValidDrop(['/a'], '/a/b')).toBe(false);
        expect(isValidDrop(['/home/user/photos'], '/home/user/photos/2024')).toBe(false);
    });

    it('accepts an unrelated destination', () => {
        expect(isValidDrop(['/a'], '/b')).toBe(true);
        expect(isValidDrop(['/home/user/photos'], '/home/user/documents')).toBe(true);
    });

    it('does not treat a sibling with a shared name prefix as a child', () => {
        // "/a" is not an ancestor of "/ab"; only a true "/a/" boundary counts.
        expect(isValidDrop(['/a'], '/ab')).toBe(true);
    });

    it('rejects when any single source disqualifies the destination', () => {
        expect(isValidDrop(['/x', '/a'], '/a/b')).toBe(false);
    });

    it('rejects an empty source set (nothing to move)', () => {
        expect(isValidDrop([], '/a')).toBe(false);
    });

    it('rejects dropping a source onto its own direct parent directory', () => {
        // "/dir/file" already lives in "/dir"; moving it there is a no-op the
        // backend rejects with AlreadyExists, so it must not be offered.
        expect(isValidDrop(['/dir/file'], '/dir')).toBe(false);
        expect(isValidDrop(['/home/user/report.txt'], '/home/user')).toBe(false);
    });

    it('rejects dropping a top-level source onto the root it lives in', () => {
        // The parent of "/a" is "/", so dropping "/a" onto "/" is a no-op.
        expect(isValidDrop(['/a'], '/')).toBe(false);
    });

    it('accepts dropping a source into a different directory', () => {
        expect(isValidDrop(['/dir/file'], '/other')).toBe(true);
    });

    it('rejects when any single source already lives in the destination', () => {
        // "/dir/b" lives in "/dir"; even paired with a movable source the drop
        // is rejected as a whole.
        expect(isValidDrop(['/elsewhere/a', '/dir/b'], '/dir')).toBe(false);
    });
});
