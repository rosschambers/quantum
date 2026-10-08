import { describe, test, expect } from 'vitest';
import { lineDiff } from './lineDiff';
import { buildRows, toSplitRows, type DiffRow } from './rows';
import { OLD_FILE_VIEWER_RS, NEW_FILE_VIEWER_RS } from './fixtures/fileViewerRsFixture';

function collapsedRows(rows: readonly DiffRow[]): Extract<DiffRow, { kind: 'collapsed' }>[] {
	return rows.filter((row): row is Extract<DiffRow, { kind: 'collapsed' }> => row.kind === 'collapsed');
}

function plainScope(scopeLine: string): string {
	const stripped = scopeLine.replace(/<[^>]*>/g, '');
	return new DOMParser().parseFromString(`<div>${stripped}</div>`, 'text/html').body.textContent ?? '';
}

describe('buildRows', () => {
	test('the file_viewer.rs fixture collapses the two long unchanged runs with the expected scopes', () => {
		const items = lineDiff(OLD_FILE_VIEWER_RS, NEW_FILE_VIEWER_RS);
		const rows = buildRows(items, OLD_FILE_VIEWER_RS, NEW_FILE_VIEWER_RS, 'rust', new Set());
		const collapsed = collapsedRows(rows);

		expect(collapsed.length).toBeGreaterThanOrEqual(2);
		expect(collapsed[0].hiddenCount).toBe(19);
		expect(plainScope(collapsed[0].scopeLine)).toBe('pub enum ViewerFileType {');
		expect(collapsed[1].hiddenCount).toBe(26);
		expect(plainScope(collapsed[1].scopeLine)).toBe(
			'pub fn viewer_file_type_for_extension(extension: &str) -> ViewerFileType {',
		);
	});

	test('expanding a key yields its hidden lines framed by two recollapse rows', () => {
		const items = lineDiff(OLD_FILE_VIEWER_RS, NEW_FILE_VIEWER_RS);
		const collapsedKey = buildRows(items, OLD_FILE_VIEWER_RS, NEW_FILE_VIEWER_RS, 'rust', new Set()).find(
			(row): row is Extract<DiffRow, { kind: 'collapsed' }> => row.kind === 'collapsed',
		)!.key;

		const expandedRows = buildRows(
			items,
			OLD_FILE_VIEWER_RS,
			NEW_FILE_VIEWER_RS,
			'rust',
			new Set([collapsedKey]),
		);

		expect(expandedRows.some((row) => row.kind === 'collapsed' && row.key === collapsedKey)).toBe(false);
		const recollapseRows = expandedRows.filter(
			(row): row is Extract<DiffRow, { kind: 'recollapse' }> => row.kind === 'recollapse' && row.key === collapsedKey,
		);
		expect(recollapseRows).toHaveLength(2);
		expect(recollapseRows[0].position).toBe('top');
		expect(recollapseRows[1].position).toBe('bottom');
		expect(recollapseRows[0].count).toBe(19);

		const topIndex = expandedRows.indexOf(recollapseRows[0]);
		const bottomIndex = expandedRows.indexOf(recollapseRows[1]);
		expect(bottomIndex).toBeGreaterThan(topIndex);
		const framedRows = expandedRows.slice(topIndex + 1, bottomIndex);
		expect(framedRows.length).toBe(19);
		expect(framedRows.every((row) => row.kind === 'context')).toBe(true);
	});

	test('a run of 2 unchanged lines never collapses', () => {
		const oldLines = ['a', 'same1', 'same2', 'b'];
		const newLines = ['x', 'same1', 'same2', 'y'];
		const items = lineDiff(oldLines, newLines);
		const rows = buildRows(items, oldLines, newLines, undefined, new Set());
		expect(rows.some((row) => row.kind === 'collapsed')).toBe(false);
		expect(rows.filter((row) => row.kind === 'context')).toHaveLength(2);
	});

	test('a run longer than the collapse threshold collapses with the correct hidden count', () => {
		// A 10-line unchanged run sandwiched between two changes: 3 lines of
		// head context and 3 of tail context are kept, leaving 10 - 3 - 3 = 4
		// hidden lines, which is above the 2-line collapse threshold.
		const common = Array.from({ length: 10 }, (_, index) => `c${index}`);
		const oldLines = ['change-old', ...common, 'change-old2'];
		const newLines = ['change-new', ...common, 'change-new2'];
		const items = lineDiff(oldLines, newLines);
		const rows = buildRows(items, oldLines, newLines, undefined, new Set());
		const collapsed = collapsedRows(rows);
		expect(collapsed).toHaveLength(1);
		expect(collapsed[0].hiddenCount).toBe(4);
		expect(collapsed[0].firstNewIndex).toBe(4);
	});

	test('pairs removed[i] with added[i] via intra-line emphasis in a change block', () => {
		const oldLines = ['let ext_lower = extension.to_lowercase();'];
		const newLines = ['let extension_lower = extension.to_ascii_lowercase();'];
		const items = lineDiff(oldLines, newLines);
		const rows = buildRows(items, oldLines, newLines, 'rust', new Set());
		const removedRow = rows.find((row): row is Extract<DiffRow, { kind: 'removed' }> => row.kind === 'removed')!;
		const addedRow = rows.find((row): row is Extract<DiffRow, { kind: 'added' }> => row.kind === 'added')!;
		expect(removedRow.emphasis).not.toBeNull();
		expect(addedRow.emphasis).not.toBeNull();
		const removedText = removedRow.emphasis!.map(([start, end]) => oldLines[0].slice(start, end));
		const addedText = addedRow.emphasis!.map(([start, end]) => newLines[0].slice(start, end));
		expect(removedText).toEqual(['ext_lower', 'to_lowercase']);
		expect(addedText).toEqual(['extension_lower', 'to_ascii_lowercase']);
	});
});

describe('toSplitRows', () => {
	test('context rows become one paired row with both halves populated', () => {
		const rows: DiffRow[] = [{ kind: 'context', oldIndex: 0, newIndex: 0 }];
		const split = toSplitRows(rows);
		expect(split).toEqual([{ left: rows[0], right: rows[0], full: false }]);
	});

	test('an uneven removed/added block fills the shorter side with null', () => {
		const rows: DiffRow[] = [
			{ kind: 'removed', oldIndex: 0, emphasis: null },
			{ kind: 'removed', oldIndex: 1, emphasis: null },
			{ kind: 'added', newIndex: 0, emphasis: null },
		];
		const split = toSplitRows(rows);
		expect(split).toEqual([
			{ left: rows[0], right: rows[2], full: false },
			{ left: rows[1], right: null, full: false },
		]);
	});

	test('collapsed and recollapse rows span the full width in the left slot', () => {
		const collapsedRow: DiffRow = { kind: 'collapsed', key: 'k', hiddenCount: 5, firstNewIndex: 2, scopeLine: '' };
		const split = toSplitRows([collapsedRow]);
		expect(split).toEqual([{ left: collapsedRow, right: null, full: true }]);
	});
});
