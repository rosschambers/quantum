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
