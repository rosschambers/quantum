import { describe, it, expect } from 'vitest';
import { parentOf, relativeGroupLabel } from './path';

describe('parentOf', () => {
    it('returns the containing directory of a nested path', () => {
        expect(parentOf('/dir/file')).toBe('/dir');
        expect(parentOf('/home/user/report.txt')).toBe('/home/user');
    });

    it('returns root for a top-level directory', () => {
        expect(parentOf('/home')).toBe('/');
    });

    it('treats the root as its own parent', () => {
        expect(parentOf('/')).toBe('/');
    });

    it('ignores a trailing slash', () => {
        expect(parentOf('/home/user/')).toBe('/home');
    });
});


describe('relativeGroupLabel', () => {
    it('labels the search root itself as "."', () => {
        expect(relativeGroupLabel('/home/user/projects', '/home/user/projects')).toBe('.');
    });

    it('strips the root prefix from group paths under it', () => {
        expect(relativeGroupLabel('/home/user/projects/quantum', '/home/user/projects')).toBe(
            'quantum',
        );
        expect(relativeGroupLabel('/a/b/c/d', '/a')).toBe('b/c/d');
    });

    it('falls back to the absolute path when the group is not under the root', () => {
        expect(relativeGroupLabel('/other/place', '/home/user/projects')).toBe('/other/place');
    });

    it('handles a root of "/"', () => {
        expect(relativeGroupLabel('/home', '/')).toBe('home');
    });
});
