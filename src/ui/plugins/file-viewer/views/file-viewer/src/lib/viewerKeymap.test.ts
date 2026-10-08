import { describe, expect, test } from 'vitest';
import { resolveViewerShortcut } from './viewerKeymap';

function key(init: Partial<KeyboardEvent>): KeyboardEvent {
    return {
        key: '', ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
        ...init,
    } as KeyboardEvent;
}

describe('resolveViewerShortcut', () => {
    test('Ctrl+F and Cmd+F open search; Enter/Shift+Enter navigate; Escape resolves', () => {
        expect(resolveViewerShortcut(key({ key: 'f', ctrlKey: true }))).toEqual({ kind: 'open-search' });
        expect(resolveViewerShortcut(key({ key: 'f', metaKey: true }))).toEqual({ kind: 'open-search' });
        expect(resolveViewerShortcut(key({ key: 'Enter' }))).toEqual({ kind: 'next-match' });
        expect(resolveViewerShortcut(key({ key: 'Enter', shiftKey: true }))).toEqual({ kind: 'previous-match' });
        expect(resolveViewerShortcut(key({ key: 'Escape' }))).toEqual({ kind: 'close-search' });
        expect(resolveViewerShortcut(key({ key: 'f', ctrlKey: true, altKey: true }))).toBeNull();
    });

    test('capital F with the control modifier still resolves (key casing is not significant)', () => {
        expect(resolveViewerShortcut(key({ key: 'F', ctrlKey: true }))).toEqual({ kind: 'open-search' });
    });

    test('Ctrl+F with shift held does not open search (reserved, mirrors the explorer convention)', () => {
        expect(resolveViewerShortcut(key({ key: 'f', ctrlKey: true, shiftKey: true }))).toBeNull();
    });

    test('non-shortcuts resolve to null', () => {
        expect(resolveViewerShortcut(key({ key: 'a' }))).toBeNull();
        expect(resolveViewerShortcut(key({ key: 'ArrowDown' }))).toBeNull();
        expect(resolveViewerShortcut(key({ key: 'f' }))).toBeNull();
    });
});

describe('resolveViewerShortcut diff-mode shortcuts', () => {
    test('n, p, ], [, s and r/R resolve to their diff-only actions, unconditionally of mode', () => {
        expect(resolveViewerShortcut(key({ key: 'n' }))).toEqual({ kind: 'next-change' });
        expect(resolveViewerShortcut(key({ key: 'p' }))).toEqual({ kind: 'previous-change' });
        expect(resolveViewerShortcut(key({ key: ']' }))).toEqual({ kind: 'next-file' });
        expect(resolveViewerShortcut(key({ key: '[' }))).toEqual({ kind: 'previous-file' });
        expect(resolveViewerShortcut(key({ key: 's' }))).toEqual({ kind: 'toggle-stage' });
        expect(resolveViewerShortcut(key({ key: 'r' }))).toEqual({ kind: 'refresh' });
        expect(resolveViewerShortcut(key({ key: 'R' }))).toEqual({ kind: 'refresh' });
    });

    test('none of the diff-only shortcuts fire with Ctrl, Meta, or Alt held', () => {
        for (const diffKey of ['n', 'p', ']', '[', 's', 'r']) {
            expect(resolveViewerShortcut(key({ key: diffKey, ctrlKey: true }))).toBeNull();
            expect(resolveViewerShortcut(key({ key: diffKey, metaKey: true }))).toBeNull();
            expect(resolveViewerShortcut(key({ key: diffKey, altKey: true }))).toBeNull();
        }
    });
});
