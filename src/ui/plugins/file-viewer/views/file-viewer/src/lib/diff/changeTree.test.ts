import { describe, expect, test } from 'vitest';
import type { ReviewEntry, ReviewSection } from './reviewModel';
import {
	buildChangeTree,
	compareTreePaths,
	entriesUnder,
	flattenChangeTree,
	sortEntriesForTree,
	type TreeRow,
} from './changeTree';

function entry(path: string, section: ReviewSection = 'unstaged'): ReviewEntry {
	return Object.freeze({ id: `${path}:${section}`, fileId: path, path, status: 'M', section, partiallyStaged: false }) as ReviewEntry;
}

const MURMUR8_PATHS = [
	'.github/workflows/runner-watchdog.yml',
	'src/Murmur8.Application/AI/ToolExecutor.cs',
	'src/Murmur8.Application/Calendar/Commands/SetCalendarNotificationRules/SetCalendarNotificationRulesCommandHandler.cs',
	'src/Murmur8.Application/Calendar/EventHandlers/NotifyCalendarNotificationRulesUpdated.cs',
	'src/Murmur8.Application/Calendar/Events/CalendarEvents.cs',
	'src/Murmur8.Web/lib/api/calendarQueryKeys.ts',
	'src/Murmur8.Web/lib/realtime/EventsContext.tsx',
	'src/Murmur8.Worker/Notifications/NotificationScheduler.cs',
	'tests/Murmur8.Application.Tests/AI/ToolExecutorTests.cs',
	'tests/Murmur8.Application.Tests/Calendar/Commands/SetCalendarNotificationRulesCommandHandlerTests.cs',
	'tests/Murmur8.Application.Tests/Calendar/NotifyCalendarNotificationRulesUpdatedTests.cs',
	'tests/Murmur8.Worker.Tests/Notifications/NotificationSchedulerRuleTests.cs',
	'tests/Murmur8.Worker.Tests/Notifications/NotificationSchedulerTests.cs',
];

const EDGE_PATHS = [
	'src/zeta.ts',
	'src/alpha/one.ts',
	'src-old.ts',
	'src.ts',
	'a/b/c/deep.ts',
	'a.ts',
	'README.md',
	'Zed/x.ts',
	'zed.ts',
	'a',
	'a/b',
	'caf\u00e9/x.ts',
	'cafe\u0301/y.ts',
];

function describeRows(rows: TreeRow[]): string[] {
	return rows.map((row) =>
		row.kind === 'folder'
			? `${'  '.repeat(row.depth)}${row.folder.name}/${row.expanded ? '' : ' (folded)'}`
			: `${'  '.repeat(row.depth)}${row.entry.path.split('/').pop()}`,
	);
}

function fileOrder(rows: TreeRow[]): string[] {
	return rows.flatMap((row) => (row.kind === 'file' ? [row.entry.path] : []));
}

describe('changeTree', () => {
	test('the murmur8 change set flattens into compacted, mixed-alphabetical rows', () => {
		const entries = Object.freeze(MURMUR8_PATHS.map((path) => entry(path)));
		expect(describeRows(flattenChangeTree(buildChangeTree(entries), new Set()))).toEqual([
			'.github/workflows/',
			'  runner-watchdog.yml',
			'src/',
			'  Murmur8.Application/',
			'    AI/',
			'      ToolExecutor.cs',
			'    Calendar/',
			'      Commands/SetCalendarNotificationRules/',
			'        SetCalendarNotificationRulesCommandHandler.cs',
			'      EventHandlers/',
			'        NotifyCalendarNotificationRulesUpdated.cs',
			'      Events/',
			'        CalendarEvents.cs',
			'  Murmur8.Web/lib/',
			'    api/',
			'      calendarQueryKeys.ts',
			'    realtime/',
			'      EventsContext.tsx',
			'  Murmur8.Worker/Notifications/',
			'    NotificationScheduler.cs',
			'tests/',
			'  Murmur8.Application.Tests/',
			'    AI/',
			'      ToolExecutorTests.cs',
			'    Calendar/',
			'      Commands/',
			'        SetCalendarNotificationRulesCommandHandlerTests.cs',
			'      NotifyCalendarNotificationRulesUpdatedTests.cs',
			'  Murmur8.Worker.Tests/Notifications/',
			'    NotificationSchedulerRuleTests.cs',
			'    NotificationSchedulerTests.cs',
		]);
	});

	test.each([
		['murmur8', MURMUR8_PATHS],
		['edge cases', EDGE_PATHS],
	])('the %s tree reads its files in compareTreePaths order', (_name, paths) => {
		const entries = Object.freeze(paths.map((path) => entry(path)));
		const expected = [...paths].sort(compareTreePaths);
		expect(fileOrder(flattenChangeTree(buildChangeTree(entries), new Set()))).toEqual(expected);
	});

	test('folders and files mix alphabetically; a folder sorts before a same-stem file', () => {
		const entries = Object.freeze(['src/zeta.ts', 'src/alpha/one.ts', 'src-old.ts', 'src.ts'].map((path) => entry(path)));
		expect(describeRows(flattenChangeTree(buildChangeTree(entries), new Set()))).toEqual([
			'src/',
			'  alpha/',
			'    one.ts',
			'  zeta.ts',
			'src-old.ts',
			'src.ts',
		]);
	});

	test('a folded folder keeps its row and hides its descendants', () => {
		const entries = Object.freeze(MURMUR8_PATHS.map((path) => entry(path)));
		const rows = flattenChangeTree(buildChangeTree(entries), new Set(['src/Murmur8.Web/lib']));
		const described = describeRows(rows);
		expect(described).toContain('  Murmur8.Web/lib/ (folded)');
		expect(described).not.toContain('      calendarQueryKeys.ts');
		expect(described).not.toContain('    api/');
	});

	test('a compacted folder is keyed by the path of its deepest folder', () => {
		const tree = buildChangeTree(Object.freeze([entry('a/b/c/deep.ts')]));
		const [folder] = tree.folders;
		expect(folder.name).toBe('a/b/c');
		expect(folder.path).toBe('a/b/c');
	});

	test('entriesUnder returns every entry below a folder in tree order', () => {
		const entries = Object.freeze(MURMUR8_PATHS.map((path) => entry(path)));
		const tree = buildChangeTree(entries);
		const tests = tree.folders.find((folder) => folder.name === 'tests');
		expect(entriesUnder(tests!).map((item) => item.path)).toEqual(MURMUR8_PATHS.filter((path) => path.startsWith('tests/')).sort(compareTreePaths));
	});

	test('compareTreePaths returns a negative, positive, or zero sign on simple pairs', () => {
		expect(compareTreePaths('a.ts', 'b.ts')).toBeLessThan(0);
		expect(compareTreePaths('b.ts', 'a.ts')).toBeGreaterThan(0);
		expect(compareTreePaths('src/a.ts', 'src/a.ts')).toBe(0);
	});

	test('compareTreePaths puts a file before a folder of the same name', () => {
		expect(compareTreePaths('a', 'a/b')).toBeLessThan(0);
		expect(compareTreePaths('a/b', 'a')).toBeGreaterThan(0);
	});

	test('canonically equal names order by code unit, not by input order', () => {
		expect(compareTreePaths('cafe\u0301/y.ts', 'caf\u00e9/x.ts')).toBeLessThan(0);
		const entries = Object.freeze([entry('caf\u00e9/x.ts'), entry('cafe\u0301/y.ts')]);
		expect(fileOrder(flattenChangeTree(buildChangeTree(entries), new Set()))).toEqual(['cafe\u0301/y.ts', 'caf\u00e9/x.ts']);
	});

	test('buildChangeTree of no entries has no folders, no files, and flattens to no rows', () => {
		const tree = buildChangeTree(Object.freeze([]));
		expect(tree.folders).toEqual([]);
		expect(tree.files).toEqual([]);
		expect(flattenChangeTree(tree, new Set())).toEqual([]);
	});

	test('collapsing an interior member of a compacted chain hides nothing', () => {
		const tree = buildChangeTree(Object.freeze([entry('a/b/c/deep.ts')]));
		expect(describeRows(flattenChangeTree(tree, new Set(['a'])))).toEqual(['a/b/c/', '  deep.ts']);
	});

	test('sortEntriesForTree keeps Unstaged before Staged and tree order within each', () => {
		const entries = Object.freeze([entry('src.ts', 'staged'), entry('src.ts'), entry('src/a.ts'), entry('src/a.ts', 'staged')]);
		expect(sortEntriesForTree(entries).map((item) => item.id)).toEqual(['src/a.ts:unstaged', 'src.ts:unstaged', 'src/a.ts:staged', 'src.ts:staged']);
	});
});
