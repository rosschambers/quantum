import { describe, test, expect } from 'vitest';
import { buildCodeFoldModel, foldContaining, foldAncestors, visibleRowOfLine } from './fold-model';

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

describe('visibleRowOfLine', () => {
    test('is the identity mapping when there are no folds', () => {
        const lines = ['a();', 'b();', 'c();'];
        const model = buildCodeFoldModel(lines);
        expect(model.size).toBe(0);
        const { rowOfLine, rowCount } = visibleRowOfLine(lines.length, model, () => false);
        expect(Array.from(rowOfLine)).toEqual([0, 1, 2]);
        expect(rowCount).toBe(3);
    });

    test('a collapsed fold maps every hidden line to the header row and shifts later rows', () => {
        const lines = ['function f() {', '  return 1;', '}', 'g();'];
        const model = buildCodeFoldModel(lines);
        const { rowOfLine, rowCount } = visibleRowOfLine(lines.length, model, (startLine) => startLine === 0);
        // Lines 0 and 1 (the header and its hidden body line) both map to row 0.
        expect(Array.from(rowOfLine)).toEqual([0, 0, 1, 2]);
        expect(rowCount).toBe(3);
    });

    test('a nested collapsed fold inside a collapsed outer fold maps to the outermost header row', () => {
        const lines = [
            'function outer() {',
            '  function inner() {',
            '    return 1;',
            '  }',
            '}',
            'g();',
        ];
        const model = buildCodeFoldModel(lines);
        // Collapsing the outer header (line 0) only; whether the inner fold is
        // independently "collapsed" never matters because the outer walk never
        // reaches it.
        const { rowOfLine, rowCount } = visibleRowOfLine(lines.length, model, (startLine) => startLine === 0);
        expect(Array.from(rowOfLine)).toEqual([0, 0, 0, 0, 1, 2]);
        expect(rowCount).toBe(3);
    });

    test('expanded folds hide nothing', () => {
        const lines = ['function f() {', '  return 1;', '}', 'g();'];
        const model = buildCodeFoldModel(lines);
        const { rowOfLine, rowCount } = visibleRowOfLine(lines.length, model, () => false);
        expect(Array.from(rowOfLine)).toEqual([0, 1, 2, 3]);
        expect(rowCount).toBe(4);
    });

    test('a malformed fold whose endLine is before its startLine does not loop forever and still terminates with a sensible mapping', () => {
        // A hand-built model (never produced by buildCodeFoldModel itself)
        // representing a corrupt fold: endLine before startLine.
        const model = new Map<number, CodeFoldRange>([[2, { startLine: 2, endLine: 0 }]]);
        const { rowOfLine, rowCount } = visibleRowOfLine(4, model, () => true);
        expect(rowOfLine).toHaveLength(4);
        // Termination alone is the behavioral fix; every line still gets a
        // row within the final row count, and the walk made forward progress
        // past the malformed entry instead of looping on it.
        expect(rowCount).toBeGreaterThan(0);
        expect(rowCount).toBeLessThanOrEqual(4);
        for (const row of rowOfLine) {
            expect(row).toBeLessThan(rowCount);
        }
    });
});
