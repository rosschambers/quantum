import { describe, expect, test } from 'vitest';
import { SHORTCUT_KEYS, SHORTCUT_GROUPS } from './shortcuts';

describe('shortcuts reference', () => {
    test('SHORTCUT_KEYS has the core actions', () => {
        expect(SHORTCUT_KEYS.copy).toBe('Ctrl+C');
        expect(SHORTCUT_KEYS.deletePermanent).toBe('Shift+Del');
        expect(SHORTCUT_KEYS.newFolder).toBe('Ctrl+Shift+N');
    });
    test('every group hint has keys and description', () => {
        expect(SHORTCUT_GROUPS.length).toBeGreaterThan(0);
        for (const group of SHORTCUT_GROUPS) {
            expect(group.hints.length).toBeGreaterThan(0);
            for (const hint of group.hints) {
                expect(hint.keys).toBeTruthy();
                expect(hint.description).toBeTruthy();
            }
        }
    });
    test('groups reference the shared key strings', () => {
        const clipboard = SHORTCUT_GROUPS.find((g) => g.title === 'Clipboard');
        const copy = clipboard?.hints.find((h) => h.description === 'Copy');
        expect(copy?.keys).toBe(SHORTCUT_KEYS.copy);
    });
});
