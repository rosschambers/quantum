import { describe, it, expect } from 'vitest';
import { parentOf } from './path';

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
