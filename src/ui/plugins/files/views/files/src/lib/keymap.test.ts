import { describe, expect, test } from 'vitest';
import { resolveShortcut } from './keymap';

function key(init: Partial<KeyboardEvent>): KeyboardEvent {
    return {
        key: '', ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
        ...init,
    } as KeyboardEvent;
}

describe('resolveShortcut', () => {
    test('Ctrl+A is select-all', () => {
        expect(resolveShortcut(key({ key: 'a', ctrlKey: true }))).toEqual({ kind: 'select-all' });
    });
    test('Cmd+A is select-all', () => {
        expect(resolveShortcut(key({ key: 'a', metaKey: true }))).toEqual({ kind: 'select-all' });
    });
    test('Ctrl+C / Ctrl+X / Ctrl+V', () => {
        expect(resolveShortcut(key({ key: 'c', ctrlKey: true }))).toEqual({ kind: 'clipboard', operation: 'copy' });
        expect(resolveShortcut(key({ key: 'x', ctrlKey: true }))).toEqual({ kind: 'clipboard', operation: 'cut' });
        expect(resolveShortcut(key({ key: 'v', ctrlKey: true }))).toEqual({ kind: 'paste' });
    });
    test('Delete vs Shift+Delete', () => {
        expect(resolveShortcut(key({ key: 'Delete' }))).toEqual({ kind: 'trash' });
        expect(resolveShortcut(key({ key: 'Delete', shiftKey: true }))).toEqual({ kind: 'delete-permanent' });
    });
    test('F2 rename, Ctrl+D duplicate, Ctrl+Shift+N new-folder, F5 refresh', () => {
        expect(resolveShortcut(key({ key: 'F2' }))).toEqual({ kind: 'rename' });
        expect(resolveShortcut(key({ key: 'd', ctrlKey: true }))).toEqual({ kind: 'duplicate' });
        expect(resolveShortcut(key({ key: 'N', ctrlKey: true, shiftKey: true }))).toEqual({ kind: 'new-folder' });
        expect(resolveShortcut(key({ key: 'F5' }))).toEqual({ kind: 'refresh' });
    });
    test('Home / End', () => {
        expect(resolveShortcut(key({ key: 'Home' }))).toEqual({ kind: 'cursor', to: 'first' });
        expect(resolveShortcut(key({ key: 'End' }))).toEqual({ kind: 'cursor', to: 'last' });
    });
    test('Ctrl+H toggle-hidden, Escape clear-selection', () => {
        expect(resolveShortcut(key({ key: 'h', ctrlKey: true }))).toEqual({ kind: 'toggle-hidden' });
        expect(resolveShortcut(key({ key: 'Escape' }))).toEqual({ kind: 'clear-selection' });
    });
    test('non-shortcuts resolve to null', () => {
        expect(resolveShortcut(key({ key: 'n', ctrlKey: true }))).toBeNull();
        expect(resolveShortcut(key({ key: 'a' }))).toBeNull();
        expect(resolveShortcut(key({ key: 'ArrowDown' }))).toBeNull();
        expect(resolveShortcut(key({ key: 'l', ctrlKey: true }))).toBeNull();
        expect(resolveShortcut(key({ key: 'ArrowLeft', altKey: true }))).toBeNull();
    });
});
