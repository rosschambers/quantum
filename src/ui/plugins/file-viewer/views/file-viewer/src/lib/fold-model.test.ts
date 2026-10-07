import { describe, test, expect } from 'vitest';
import { buildCodeFoldModel, foldContaining, foldAncestors } from './fold-model';

describe('foldContaining', () => {
    test('finds the fold containing a line, not its header', () => {
        const model = buildCodeFoldModel(['function f() {', '  return 1;', '}', 'g();']);
        expect(foldContaining(model, 1)).toEqual({ startLine: 0, endLine: 1 });
        expect(foldContaining(model, 0)).toBeNull();
    });

    test('returns null for a line with no containing fold', () => {
        const model = buildCodeFoldModel(['function f() {', '  return 1;', '}', 'g();']);
        expect(foldContaining(model, 3)).toBeNull();
    });

    test('for a line nested inside multiple folds, returns the innermost one', () => {
        const lines = [
            'function outer() {',
            '  function inner() {',
            '    return 1;',
            '  }',
            '}',
        ];
        const model = buildCodeFoldModel(lines);
        // Line 2 ("return 1;") sits inside both the outer fold (0-3, per
        // buildCodeFoldModel's own indentation-based end-detection: a fold's
        // endLine is the LAST line with greater indent than its header, which
        // for the outer function is its inner closing brace at line 3, not
        // the outer closing brace at line 4) and the inner fold (1-2). The
        // innermost (tightest) one is expected.
        expect(foldContaining(model, 2)).toEqual({ startLine: 1, endLine: 2 });
    });
});

describe('foldAncestors', () => {
    test('returns every fold enclosing a nested line, outermost first', () => {
        const lines = [
            'function outer() {',
            '  function inner() {',
            '    return 1;',
            '  }',
            '}',
        ];
        const model = buildCodeFoldModel(lines);
        expect(foldAncestors(model, 2)).toEqual([
            { startLine: 0, endLine: 3 },
            { startLine: 1, endLine: 2 },
        ]);
    });

    test('returns an empty array when no fold encloses the line', () => {
        const model = buildCodeFoldModel(['function f() {', '  return 1;', '}', 'g();']);
        expect(foldAncestors(model, 3)).toEqual([]);
    });

    test("excludes a fold whose own header is the line itself", () => {
        const model = buildCodeFoldModel(['function f() {', '  return 1;', '}', 'g();']);
        expect(foldAncestors(model, 0)).toEqual([]);
    });
});
