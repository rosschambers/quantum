# qv diff sidebar folder tree view: implementation plan

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** The qv diff Changes sidebar shows changed files as a folder tree by default (key `t` or the
title button switches to the flat list), with the main pane following the tree order and a folder
checkbox that stages or unstages everything under it.

**Architecture:** A new pure module `changeTree.ts` builds, compacts, orders and flattens a folder tree
over one section's `ReviewEntry` list. `ChangesSidebar.svelte` renders it when given `view: 'tree'`
(its default stays `'list'`, so existing callers and tests are unchanged). `DiffView.svelte` owns the
view state, re-orders `entries` into tree order for git sources so the pane, `activeIndex`, `]`/`[`
and scroll tracking follow the sidebar, and adds sequential folder staging on the existing per-file
IPC.

**Tech Stack:** Svelte 5 (runes), TypeScript, vitest 2 + jsdom + `@testing-library/svelte/svelte5`.

**Assumptions:**
- Design: `docs/plans/2026-10-13-qv-diff-tree-view-design.md` (approved 2026-10-13). Settled
  choices are in its table; this plan does not revisit them.
- Git change-set paths are repository-relative with `/` separators.
- `String.prototype.localeCompare` with no locale, as `reviewModel.ts` already uses. Verified in node:
  `'.github'.localeCompare('src') === -1`, `'EventHandlers'.localeCompare('Events') === -1`,
  `'a'.localeCompare('a.ts') === -1`.
- All commands run from the view directory unless stated:
  `cd src/ui/plugins/file-viewer/views/file-viewer` and prefix with `../../../../../../scripts/devsh.sh`
  when not already inside the nix shell. Shorthand below: `VIEW=src/ui/plugins/file-viewer/views/file-viewer`
  and `RUN="./scripts/devsh.sh bash -c"` from the repo root.
- No Rust, IPC, or `@quantum/client` change.

---

### Task 1: `t` resolves to a toggle-tree shortcut

**Files:**
- Modify: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/viewerKeymap.ts`
- Test: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/viewerKeymap.test.ts`

**Acceptance Criteria:**
1. `resolveViewerShortcut(key({ key: 't' }))` returns `{ kind: 'toggle-tree' }`.
2. `t` with Ctrl, Meta, or Alt held returns `null`.
3. `ViewerShortcutAction` includes `{ kind: 'toggle-tree' }`.
4. No other file changes.

**Step 1: Write the failing test.** In `viewerKeymap.test.ts`, inside `describe('resolveViewerShortcut diff-mode shortcuts', ...)`:
- add to the first test: `expect(resolveViewerShortcut(key({ key: 't' }))).toEqual({ kind: 'toggle-tree' });`
- change the modifier loop's list to `['n', 'p', ']', '[', 's', 'r', 't']`.

**Step 2: Run it, expect failure.**
`$RUN "cd $VIEW && pnpm exec vitest run src/lib/viewerKeymap.test.ts"`. Expected: FAIL, received `null`.

**Step 3: Implement.** In `viewerKeymap.ts` add `| { kind: 'toggle-tree' }` after `toggle-stage` in the
union (comment: `// Switches the diff Changes sidebar between tree and flat list.`), and in the final
`switch (event.key)` add:

```ts
        case 't':
            return { kind: 'toggle-tree' };
```

**Step 4: Run it, expect pass.** Same command. Expected: PASS.

**Step 5: Commit.**
```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/viewerKeymap.ts src/ui/plugins/file-viewer/views/file-viewer/src/lib/viewerKeymap.test.ts
git commit -m "feat: resolve t to the diff sidebar tree toggle shortcut"
```

---

### Task 2: `changeTree.ts`, the pure tree core

**Files:**
- Create: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/changeTree.ts`
- Test: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/changeTree.test.ts`

**Acceptance Criteria:**
1. Exports `SidebarView`, `FolderNode`, `TreeRow`, `compareTreePaths`, `sortEntriesForTree`,
   `buildChangeTree`, `flattenChangeTree`, `entriesUnder` with the signatures below.
2. On the 13-path murmur8 fixture, `flattenChangeTree(buildChangeTree(entries), new Set())` produces
   exactly the row list in the test (names, kinds, depths).
3. For the murmur8 fixture and the edge-case fixture, the file rows of the flattened tree are in the
   same order as `[...entries].sort((left, right) => compareTreePaths(left.path, right.path))`.
4. A collapsed folder path emits the folder row with `expanded: false` and none of its descendants.
5. `entriesUnder` returns every entry below a folder, in tree order.
6. `sortEntriesForTree` keeps Unstaged before Staged and orders within a section by `compareTreePaths`.
7. No input array or entry is mutated (fixtures are `Object.freeze`d in the test).

**Step 1: Write the failing test** `changeTree.test.ts`:

```ts
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

const EDGE_PATHS = ['src/zeta.ts', 'src/alpha/one.ts', 'src-old.ts', 'src.ts', 'a/b/c/deep.ts', 'a.ts', 'README.md', 'Zed/x.ts', 'zed.ts'];

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

	test('sortEntriesForTree keeps Unstaged before Staged and tree order within each', () => {
		const entries = Object.freeze([entry('src.ts', 'staged'), entry('src.ts'), entry('src/a.ts'), entry('src/a.ts', 'staged')]);
		expect(sortEntriesForTree(entries).map((item) => item.id)).toEqual(['src/a.ts:unstaged', 'src.ts:unstaged', 'src/a.ts:staged', 'src.ts:staged']);
	});
});
```

**Step 2: Run it, expect failure.**
`$RUN "cd $VIEW && pnpm exec vitest run src/lib/diff/changeTree.test.ts"`. Expected: FAIL, cannot resolve `./changeTree`.

**Step 3: Implement** `changeTree.ts`:

```ts
// Folder tree over the review entries of one sidebar section, for the qv diff
// Changes sidebar's tree view (docs/plans/2026-10-13-qv-diff-tree-view-design.md).
// Pure and Svelte-free, in the task-manager tree.ts style: build the tree,
// compact chains of folders that hold no files and a single child folder,
// order folders and files together alphabetically, and flatten into rows with
// a depth. `compareTreePaths` is that same order on bare paths, so the main
// pane can be sorted to match the sidebar without building a tree.
import type { ReviewEntry } from './reviewModel';

export type SidebarView = 'tree' | 'list';

export interface FolderNode {
	/** Display name: one path segment, or several joined by '/' after compaction. */
	name: string;
	/** Full path of the deepest folder this node stands for; '' for the root. Folding keys on it. */
	path: string;
	/** The first segment of `name`, used to order this folder among its siblings. */
	sortName: string;
	folders: FolderNode[];
	files: ReviewEntry[];
}

export type TreeRow =
	| { kind: 'folder'; depth: number; folder: FolderNode; expanded: boolean }
	| { kind: 'file'; depth: number; entry: ReviewEntry };

interface MutableFolder {
	name: string;
	path: string;
	folders: Map<string, MutableFolder>;
	files: ReviewEntry[];
}

type TreeChild = { kind: 'folder'; folder: FolderNode; sortName: string } | { kind: 'file'; entry: ReviewEntry; sortName: string };

function baseName(path: string): string {
	return path.slice(path.lastIndexOf('/') + 1);
}

/** Orders two paths the way the tree reads them: segment by segment, folders and files mixed. */
export function compareTreePaths(left: string, right: string): number {
	const leftSegments = left.split('/');
	const rightSegments = right.split('/');
	const sharedLength = Math.min(leftSegments.length, rightSegments.length);
	for (let index = 0; index < sharedLength; index++) {
		const order = leftSegments[index].localeCompare(rightSegments[index]);
		if (order !== 0) {
			return order;
		}
	}
	return leftSegments.length - rightSegments.length;
}

function sectionRank(entry: ReviewEntry): number {
	return entry.section === 'staged' ? 1 : 0;
}

/** Unstaged before Staged (as `reviewEntries` does), then tree order within each section. */
export function sortEntriesForTree(entries: readonly ReviewEntry[]): ReviewEntry[] {
	return [...entries].sort((left, right) => sectionRank(left) - sectionRank(right) || compareTreePaths(left.path, right.path));
}

function compactFolder(folder: MutableFolder): FolderNode {
	let current = folder;
	let name = folder.name;
	while (current.files.length === 0 && current.folders.size === 1) {
		const [onlyChild] = current.folders.values();
		name = `${name}/${onlyChild.name}`;
		current = onlyChild;
	}
	return toFolderNode(current, name, folder.name);
}

function toFolderNode(folder: MutableFolder, name: string, sortName: string): FolderNode {
	return {
		name,
		path: folder.path,
		sortName,
		folders: [...folder.folders.values()].map(compactFolder),
		files: [...folder.files],
	};
}

/** Groups one section's entries into a folder tree whose single-child folder chains are compacted. */
export function buildChangeTree(entries: readonly ReviewEntry[]): FolderNode {
	const root: MutableFolder = { name: '', path: '', folders: new Map(), files: [] };
	for (const entry of entries) {
		const segments = entry.path.split('/');
		let folder = root;
		for (const segment of segments.slice(0, -1)) {
			let child = folder.folders.get(segment);
			if (child === undefined) {
				child = { name: segment, path: folder.path === '' ? segment : `${folder.path}/${segment}`, folders: new Map(), files: [] };
				folder.folders.set(segment, child);
			}
			folder = child;
		}
		folder.files.push(entry);
	}
	return toFolderNode(root, '', '');
}

function orderedChildren(folder: FolderNode): TreeChild[] {
	const children: TreeChild[] = [
		...folder.folders.map((child): TreeChild => ({ kind: 'folder', folder: child, sortName: child.sortName })),
		...folder.files.map((entry): TreeChild => ({ kind: 'file', entry, sortName: baseName(entry.path) })),
	];
	return children.sort((left, right) => left.sortName.localeCompare(right.sortName));
}

/** Flattens a tree into display rows; a folder whose path is in `collapsedPaths` hides its descendants. */
export function flattenChangeTree(root: FolderNode, collapsedPaths: ReadonlySet<string>): TreeRow[] {
	const rows: TreeRow[] = [];
	function visit(folder: FolderNode, depth: number): void {
		for (const child of orderedChildren(folder)) {
			if (child.kind === 'folder') {
				const expanded = !collapsedPaths.has(child.folder.path);
				rows.push({ kind: 'folder', depth, folder: child.folder, expanded });
				if (expanded) {
					visit(child.folder, depth + 1);
				}
			} else {
				rows.push({ kind: 'file', depth, entry: child.entry });
			}
		}
	}
	visit(root, 0);
	return rows;
}

/** Every entry below `folder`, in tree order. */
export function entriesUnder(folder: FolderNode): ReviewEntry[] {
	const result: ReviewEntry[] = [];
	function visit(current: FolderNode): void {
		for (const child of orderedChildren(current)) {
			if (child.kind === 'folder') {
				visit(child.folder);
			} else {
				result.push(child.entry);
			}
		}
	}
	visit(folder);
	return result;
}
```

**Step 4: Run it, expect pass.** Same command. Expected: PASS, 8 tests (the `test.each` counts twice).
If the edge-case ordering test fails, the comparator and the tree disagree: fix the code, never the
expectation, since agreement is the whole point.

**Step 5: Commit.**
```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/changeTree.ts src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/changeTree.test.ts
git commit -m "feat: pure folder tree for the diff Changes sidebar"
```

---

### Task 3: `ChangesSidebar.svelte` renders the tree, folds folders, has the toggle button

**Files:**
- Modify: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/ChangesSidebar.svelte`
- Test: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/ChangesSidebar.test.ts`

**Acceptance Criteria:**
1. New optional props: `view?: SidebarView` (default `'list'`), `onToggleView?: () => void`,
   `onStageMany?: (entries: ReviewEntry[]) => void`, `onUnstageMany?: (entries: ReviewEntry[]) => void`.
2. With `view` omitted or `'list'`, the rendered markup is today's (all 16 existing tests pass unchanged).
3. With `view: 'tree'`, each group (or the single non-stageable list) renders `flattenChangeTree` rows:
   folder rows are `li.folder-row` with a `button[aria-expanded]`, `.chevron` and `.folder-name`
   (text `<name>/` inside `<bdi>`); file rows are `li.entry.tree` with no `.directory` line, except
   the `renamed from <name>` line for renames.
4. Indent: the row button's inline style is `padding-left: <depth × 16>px`.
5. Clicking a folder row folds it (descendants gone, `aria-expanded="false"`) and does not call `onSelect`;
   folding is per group: folding `src` in Unstaged leaves Staged's `src` open.
6. A folded folder that contains `entries[activeIndex]` has the class `active`.
7. When `onToggleView` is given, the title shows `button.view-toggle` with `aria-pressed` equal to
   `view === 'tree'` and `title` `Show as list (t)` (tree) or `Show as tree (t)` (list); clicking calls it.
8. Sidebar width is 260px.

**Step 1: Write the failing tests.** Append to `ChangesSidebar.test.ts`, reusing its `entry()` helper:

```ts
describe('ChangesSidebar tree view', () => {
	const treeEntries = (): ReviewEntry[] => [
		entry({ path: '.github/workflows/runner-watchdog.yml', section: 'unstaged' }),
		entry({ path: 'src/Murmur8.Web/lib/api/calendarQueryKeys.ts', section: 'unstaged' }),
		entry({ path: 'src/Murmur8.Web/lib/realtime/EventsContext.tsx', section: 'unstaged' }),
		entry({ path: 'src/Murmur8.Web/lib/realtime/EventsContext.tsx', section: 'staged' }),
	];

	function folderNames(container: HTMLElement): string[] {
		return Array.from(container.querySelectorAll('.folder-row .folder-name')).map((element) => element.textContent ?? '');
	}

	test('list stays the default: no folder rows without view="tree"', () => {
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn() } });
		expect(container.querySelectorAll('.folder-row')).toHaveLength(0);
		expect(container.querySelectorAll('.entry .directory').length).toBeGreaterThan(0);
	});

	test('tree view shows compacted folders per group and one-line file rows', () => {
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), view: 'tree' } });
		expect(folderNames(container)).toEqual(['.github/workflows/', 'src/Murmur8.Web/lib/', 'api/', 'realtime/', 'src/Murmur8.Web/lib/realtime/']);
		expect(container.querySelectorAll('.entry.tree .directory')).toHaveLength(0);
	});

	test('rows indent 16px per depth', () => {
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), view: 'tree' } });
		const calendarRow = Array.from(container.querySelectorAll('.entry.tree')).find((element) => element.textContent?.includes('calendarQueryKeys.ts'));
		expect(calendarRow?.querySelector('button')?.getAttribute('style')).toContain('padding-left: 32px');
	});

	test('clicking a folder folds it in its own group only, without selecting', async () => {
		const onSelect = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect, view: 'tree' } });
		const unstagedSource = container.querySelector<HTMLButtonElement>('.folder-row button[data-group="unstaged"][data-folder="src/Murmur8.Web/lib"]');
		await fireEvent.click(unstagedSource!);
		expect(unstagedSource?.getAttribute('aria-expanded')).toBe('false');
		expect(folderNames(container)).toEqual(['.github/workflows/', 'src/Murmur8.Web/lib/', 'src/Murmur8.Web/lib/realtime/']);
		expect(onSelect).not.toHaveBeenCalled();
	});

	test('a folded folder holding the active file is marked active', async () => {
		// Index 1 is calendarQueryKeys.ts (entries are passed in display order).
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 1, onSelect: vi.fn(), view: 'tree' } });
		const folder = container.querySelector<HTMLButtonElement>('.folder-row button[data-group="unstaged"][data-folder="src/Murmur8.Web/lib"]');
		await fireEvent.click(folder!);
		expect(folder?.closest('.folder-row')?.classList.contains('active')).toBe(true);
	});

	test('the title toggle reflects the view and calls onToggleView', async () => {
		const onToggleView = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), view: 'tree', onToggleView } });
		const toggle = container.querySelector<HTMLButtonElement>('.sidebar-title .view-toggle');
		expect(toggle?.getAttribute('aria-pressed')).toBe('true');
		expect(toggle?.getAttribute('title')).toBe('Show as list (t)');
		await fireEvent.click(toggle!);
		expect(onToggleView).toHaveBeenCalledTimes(1);
	});

	test('the sidebar is 260px wide', () => {
		expect(sidebarSource).toMatch(/\.changes-sidebar\s*\{[^}]*width:\s*260px/);
	});
});
```

**Step 2: Run, expect failure.**
`$RUN "cd $VIEW && pnpm exec vitest run src/lib/diff/ChangesSidebar.test.ts"`. Expected: the new
tree tests FAIL (no `.folder-row`), the old 16 PASS.

**Step 3: Implement** in `ChangesSidebar.svelte`:

Script additions (after the existing imports and `Props`):

```ts
	import {
		buildChangeTree,
		entriesUnder,
		flattenChangeTree,
		type FolderNode,
		type SidebarView,
		type TreeRow,
	} from './changeTree';
```

Extend `Props` and the destructure:

```ts
	interface Props {
		entries: ReviewEntry[];
		stageable: boolean;
		activeIndex: number;
		onSelect: (index: number) => void;
		onStage?: (entry: ReviewEntry) => void;
		onUnstage?: (entry: ReviewEntry) => void;
		/** 'tree' groups each section's files into folders; 'list' is the original flat list. */
		view?: SidebarView;
		onToggleView?: () => void;
		/** Folder checkbox in the Unstaged tree: every entry below the folder, in tree order. */
		onStageMany?: (entries: ReviewEntry[]) => void;
		/** Folder checkbox in the Staged tree: every entry below the folder, in tree order. */
		onUnstageMany?: (entries: ReviewEntry[]) => void;
	}

	let { entries, stageable, activeIndex, onSelect, onStage, onUnstage, view = 'list', onToggleView, onStageMany, onUnstageMany }: Props = $props();
```

Below `toggleGroup`, add:

```ts
	type TreeGroup = SidebarGroup | 'none';
	// Folded folders, keyed `${group}|${folder.path}` so the same folder can be
	// folded in Unstaged and open in Staged. Navigation only, like the groups.
	let collapsedFolders: Set<string> = $state(new Set());

	function folderKey(group: TreeGroup, folder: FolderNode): string {
		return `${group}|${folder.path}`;
	}

	function toggleFolder(group: TreeGroup, folder: FolderNode): void {
		const key = folderKey(group, folder);
		const next = new Set(collapsedFolders);
		if (next.has(key)) {
			next.delete(key);
		} else {
			next.add(key);
		}
		collapsedFolders = next;
	}

	function treeRows(group: TreeGroup, items: IndexedEntry[]): TreeRow[] {
		const prefix = `${group}|`;
		const collapsedPaths = new Set(
			[...collapsedFolders].filter((key) => key.startsWith(prefix)).map((key) => key.slice(prefix.length)),
		);
		return flattenChangeTree(buildChangeTree(items.map((item) => item.entry)), collapsedPaths);
	}

	let indexedById = $derived(new Map(indexedEntries.map((item) => [item.entry.id, item])));
	let activeEntryId = $derived(entries[activeIndex]?.id);

	function handleFolderCheckbox(event: Event, group: TreeGroup, folder: FolderNode): void {
		event.stopPropagation();
		const below = entriesUnder(folder);
		if (group === 'staged') {
			onUnstageMany?.(below);
		} else {
			onStageMany?.(below);
		}
	}

	function handleFolderCheckboxKeydown(event: KeyboardEvent, group: TreeGroup, folder: FolderNode): void {
		if (event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			handleFolderCheckbox(event, group, folder);
		}
	}
```

Change the `entryRow` snippet signature to take an optional depth and use it (tree rows: one line,
indent, `tree` class); keep everything else as it is:

```svelte
{#snippet entryRow(item: IndexedEntry, depth: number | null = null)}
	{@const current = splitPath(item.entry.path)}
	<li class="entry" class:tree={depth !== null} class:active={item.index === activeIndex}>
		{#if stageable}
			<input
				type="checkbox"
				class="checkbox"
				class:checked={item.entry.section === 'staged'}
				checked={item.entry.section === 'staged'}
				aria-label={`${item.entry.section === 'staged' ? 'Unstage' : 'Stage'} ${current.name}`}
				onclick={(event) => handleCheckboxClick(event, item.entry)}
				onkeydown={(event) => handleCheckboxKeydown(event, item.entry)}
			/>
		{/if}
		<button type="button" style={depth !== null ? `padding-left: ${depth * 16}px` : undefined} onclick={() => onSelect(item.index)}>
			<span class="status status-{item.entry.status}">{item.entry.status}</span>
			<span class="name-block">
				<span class="name-line">
					<span class="filename">{current.name}</span>
					{#if item.entry.partiallyStaged}
						<span class="dot" title="partially staged: edited after staging"></span>
					{/if}
				</span>
				{#if item.entry.oldPath}
					<span class="directory rename">renamed from {splitPath(item.entry.oldPath).name}</span>
				{:else if depth === null}
					<span class="directory">{current.directory}</span>
				{/if}
			</span>
			<span class="mini-stats">+{item.additions} &#x2212;{item.deletions}</span>
		</button>
	</li>
{/snippet}

{#snippet folderRow(group: TreeGroup, row: Extract<TreeRow, { kind: 'folder' }>)}
	<li
		class="folder-row"
		class:active={!row.expanded && activeEntryId !== undefined && entriesUnder(row.folder).some((entry) => entry.id === activeEntryId)}
	>
		{#if stageable}
			<input
				type="checkbox"
				class="checkbox"
				class:checked={group === 'staged'}
				checked={group === 'staged'}
				aria-label={`${group === 'staged' ? 'Unstage' : 'Stage'} everything in ${row.folder.name}`}
				onclick={(event) => handleFolderCheckbox(event, group, row.folder)}
				onkeydown={(event) => handleFolderCheckboxKeydown(event, group, row.folder)}
			/>
		{/if}
		<button
			type="button"
			data-group={group}
			data-folder={row.folder.path}
			aria-expanded={row.expanded}
			style={`padding-left: ${row.depth * 16}px`}
			onclick={() => toggleFolder(group, row.folder)}
		>
			<span class="chevron">{row.expanded ? '\u25BC' : '\u25B6'}</span>
			<span class="folder-name"><bdi>{row.folder.name}/</bdi></span>
		</button>
	</li>
{/snippet}

{#snippet groupBody(group: TreeGroup, items: IndexedEntry[])}
	{#if view === 'tree'}
		{#each treeRows(group, items) as row (row.kind === 'folder' ? `folder:${row.folder.path}` : row.entry.id)}
			{#if row.kind === 'folder'}
				{@render folderRow(group, row)}
			{:else}
				{@render entryRow(indexedById.get(row.entry.id)!, row.depth)}
			{/if}
		{/each}
	{:else}
		{#each items as item (item.entry.id)}
			{@render entryRow(item)}
		{/each}
	{/if}
{/snippet}
```

Replace the `<nav>` body:

```svelte
<nav class="changes-sidebar">
	<div class="sidebar-title">
		<span>Changes</span>
		<span class="count">{fileCount} files</span>
		{#if onToggleView}
			<button
				type="button"
				class="view-toggle"
				aria-pressed={view === 'tree'}
				title={view === 'tree' ? 'Show as list (t)' : 'Show as tree (t)'}
				onclick={onToggleView}
			>{view === 'tree' ? '\u2261' : '\u2514'}</button>
		{/if}
	</div>
	<ul class="file-list">
		{#if stageable}
			{@render groupHeading('unstaged', 'Unstaged', unstagedEntries.length)}
			{#if !collapsedGroups.has('unstaged')}
				{@render groupBody('unstaged', unstagedEntries)}
			{/if}
			{@render groupHeading('staged', 'Staged', stagedEntries.length)}
			{#if !collapsedGroups.has('staged')}
				{@render groupBody('staged', stagedEntries)}
			{/if}
		{:else}
			{@render groupBody('none', indexedEntries)}
		{/if}
	</ul>
</nav>
```

Note: the title used to be `Changes <span class="count">`; it is now two spans. Check the existing
tests that read the title text still pass (they match on `13 files`-style substrings).

Style changes: `.changes-sidebar { width: 260px; ... }`; `.sidebar-title` gains `align-items: center;
gap: 6px;` and loses `justify-content: space-between`; `.sidebar-title .count` gains `margin-left: auto;`.
Add:

```css
	.view-toggle {
		width: 22px;
		height: 20px;
		padding: 0;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		background: none;
		color: var(--color-muted);
		cursor: pointer;
		font-size: 12px;
		line-height: 1;
	}

	.view-toggle:hover {
		color: var(--color-fg);
	}

	.entry.tree button {
		padding-top: 4px;
		padding-bottom: 4px;
	}

	.folder-row {
		display: grid;
		grid-template-columns: auto 1fr;
		align-items: center;
		gap: 6px;
		padding: 0 12px;
	}

	.folder-row button {
		width: 100%;
		min-width: 0;
		border: none;
		background: none;
		text-align: left;
		padding-top: 4px;
		padding-bottom: 4px;
		cursor: pointer;
		display: flex;
		align-items: baseline;
		gap: 6px;
		color: var(--color-muted);
		font-family: inherit;
		font-size: 12px;
	}

	.folder-row button:hover {
		color: var(--color-fg);
	}

	.folder-row.active button {
		color: var(--color-accent);
		font-weight: 600;
	}

	/* Truncate from the left so the deepest folder stays visible; the <bdi>
	   keeps the trailing '/' at the end under direction: rtl. */
	.folder-name {
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		direction: rtl;
		text-align: left;
	}
```

Important: the existing `.entry button { padding: 7px 0; }` shorthand would override the inline
`padding-left` only if it were inline too; inline style wins, so tree rows get both. Do not change
the list rows' padding.

**Step 4: Run, expect pass.** Same command. Expected: all ChangesSidebar tests PASS (16 old + 7 new).

**Step 5: Commit.**
```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/ChangesSidebar.svelte src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/ChangesSidebar.test.ts
git commit -m "feat: folder tree view in the diff Changes sidebar"
```

---

### Task 4: folder checkbox calls `onStageMany` / `onUnstageMany`

**Files:**
- Test: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/ChangesSidebar.test.ts`
- (Implementation already landed in Task 3; this task proves it and fixes anything the tests expose.)

**Acceptance Criteria:**
1. Clicking an Unstaged folder checkbox calls `onStageMany` once with every entry below it, in tree
   order, and never calls `onSelect`, `onStage` or `onUnstage`.
2. Clicking a Staged folder checkbox calls `onUnstageMany` once with the entries below it.
3. Space on a focused folder checkbox does the same as a click.
4. A non-stageable tree has no checkboxes.

**Step 1: Write the tests** (append inside `describe('ChangesSidebar tree view', ...)`):

```ts
	test('an Unstaged folder checkbox stages every file below it, in tree order, without selecting', async () => {
		const onStageMany = vi.fn();
		const onSelect = vi.fn();
		const onStage = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect, onStage, onStageMany, view: 'tree' } });
		const folderButton = container.querySelector('.folder-row button[data-group="unstaged"][data-folder="src/Murmur8.Web/lib"]');
		const checkbox = folderButton!.closest('.folder-row')!.querySelector<HTMLInputElement>('.checkbox');
		await fireEvent.click(checkbox!);
		expect(onStageMany).toHaveBeenCalledTimes(1);
		expect(onStageMany.mock.calls[0][0].map((item: ReviewEntry) => item.id)).toEqual([
			'src/Murmur8.Web/lib/api/calendarQueryKeys.ts:unstaged',
			'src/Murmur8.Web/lib/realtime/EventsContext.tsx:unstaged',
		]);
		expect(onSelect).not.toHaveBeenCalled();
		expect(onStage).not.toHaveBeenCalled();
	});

	test('a Staged folder checkbox unstages every file below it', async () => {
		const onUnstageMany = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), onUnstageMany, view: 'tree' } });
		const folderButton = container.querySelector('.folder-row button[data-group="staged"]');
		const checkbox = folderButton!.closest('.folder-row')!.querySelector<HTMLInputElement>('.checkbox');
		await fireEvent.click(checkbox!);
		expect(onUnstageMany.mock.calls[0][0].map((item: ReviewEntry) => item.id)).toEqual(['src/Murmur8.Web/lib/realtime/EventsContext.tsx:staged']);
	});

	test('Space on a folder checkbox stages like a click', async () => {
		const onStageMany = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), onStageMany, view: 'tree' } });
		const checkbox = container.querySelector<HTMLInputElement>('.folder-row .checkbox');
		await fireEvent.keyDown(checkbox!, { key: ' ' });
		expect(onStageMany).toHaveBeenCalledTimes(1);
	});

	test('a non-stageable tree has no checkboxes', () => {
		const entries = treeEntries().filter((item) => item.section === 'unstaged').map((item) => ({ ...item, section: 'none' as const, id: `${item.path}:none` }));
		const { container } = render(ChangesSidebar, { props: { entries, stageable: false, activeIndex: 0, onSelect: vi.fn(), view: 'tree' } });
		expect(container.querySelectorAll('.folder-row')).not.toHaveLength(0);
		expect(container.querySelectorAll('.checkbox')).toHaveLength(0);
	});
```

**Step 2: Run.** `$RUN "cd $VIEW && pnpm exec vitest run src/lib/diff/ChangesSidebar.test.ts"`.
Expected: PASS. If any FAIL, the Task 3 implementation is wrong; fix it, never the expectation.

**Step 3: Commit.**
```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/ChangesSidebar.test.ts
git commit -m "test: folder checkbox stages and unstages everything below it"
```

---

### Task 5: `DiffView.svelte` opens in tree view, `t` toggles, the pane follows the tree

**Files:**
- Modify: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/DiffView.svelte`
- Test: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/DiffView.test.ts`

**Acceptance Criteria:**
1. A git diff opens with `.folder-row` elements in the sidebar.
2. In tree view the pane's `.diff-file .path` order equals `compareTreePaths` order; after `t` it equals
   today's list order. Fixture paths `src/zeta.ts, src/alpha/one.ts, src-old.ts, src.ts`: tree
   `src/alpha/one.ts, src/zeta.ts, src-old.ts, src.ts`; list `src-old.ts, src.ts, src/alpha/one.ts, src/zeta.ts`.
3. `t` does nothing while focus is in an input (the search field).
4. After `t`, the previously active file is still `.entry.active` in the sidebar.
5. Pair mode (`qv <a> <b>`) is unaffected (the existing pair-mode tests pass).
6. All existing DiffView tests pass.

**Step 1: Write the failing tests** (append inside `describe('DiffView', ...)`):

```ts
	function treeOrderFixture(): ChangeSet {
		const file = (path: string, letter: string): ChangedFile => ({
			path, language: 'typescript', base: side(`old ${letter}\n`, `${letter}-base`), index: side(`old ${letter}\n`, `${letter}-base`), target: side(`new ${letter}\n`, `${letter}-target`), untracked: false,
		});
		return changeSetFixture({ files: [file('src/zeta.ts', 'z'), file('src/alpha/one.ts', 'o'), file('src-old.ts', 'd'), file('src.ts', 's')] });
	}

	function panePaths(container: HTMLElement): string[] {
		return Array.from(container.querySelectorAll('.diff-file .path')).map((element) => element.textContent?.replace(/\s+/g, '') ?? '');
	}

	test('opens in tree view with the pane in tree order, and t switches both to the list', async () => {
		mockChangesAndFingerprint(treeOrderFixture());
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelectorAll('.diff-file')).toHaveLength(4));

		expect(container.querySelectorAll('.folder-row').length).toBeGreaterThan(0);
		expect(panePaths(container)).toEqual(['src/alpha/one.ts', 'src/zeta.ts', 'src-old.ts', 'src.ts']);

		await fireEvent.keyDown(window, { key: 't' });
		await vi.waitFor(() => expect(container.querySelectorAll('.folder-row')).toHaveLength(0));
		expect(panePaths(container)).toEqual(['src-old.ts', 'src.ts', 'src/alpha/one.ts', 'src/zeta.ts']);
	});

	test('t keeps the active file active across the switch', async () => {
		mockChangesAndFingerprint(treeOrderFixture());
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelectorAll('.diff-file')).toHaveLength(4));

		await fireEvent.keyDown(window, { key: ']' }); // tree order: second file, src/zeta.ts
		await fireEvent.keyDown(window, { key: 't' });
		await vi.waitFor(() => expect(container.querySelectorAll('.folder-row')).toHaveLength(0));
		expect(container.querySelector('.entry.active .filename')?.textContent).toBe('zeta.ts');
	});

	test('t is ignored while typing in an input', async () => {
		mockChangesAndFingerprint(treeOrderFixture());
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelectorAll('.diff-file')).toHaveLength(4));

		const input = document.createElement('input');
		container.appendChild(input);
		await fireEvent.keyDown(input, { key: 't' });
		expect(container.querySelectorAll('.folder-row').length).toBeGreaterThan(0);
	});
```

`.path` renders through `{@html pathHtml}`; if its text carries separators other than the path,
compare against `entry.path` read from a `data-path` attribute instead (add none to production for
this; adjust the helper).

**Step 2: Run, expect failure.**
`$RUN "cd $VIEW && pnpm exec vitest run src/lib/diff/DiffView.test.ts"`. Expected: the three new tests FAIL.

**Step 3: Implement** in `DiffView.svelte`:
- Imports: `import { tick } from 'svelte';` and
  `import { sortEntriesForTree, type SidebarView } from './changeTree';`
- State next to `layout`: `let sidebarView: SidebarView = $state('tree');`
- Replace `let entries = $derived(changeSet ? reviewEntries(changeSet) : []);` with:

```ts
	// Tree view reads the pane in the sidebar's tree order, so ] and [ and the
	// scroll-tracked active file walk the tree. Pair mode has no sidebar and
	// keeps its order.
	let entries = $derived.by(() => {
		const reviewed = changeSet ? reviewEntries(changeSet) : [];
		return sidebarView === 'tree' && source.kind === 'git' ? sortEntriesForTree(reviewed) : reviewed;
	});
```

- After `goToFile`, add:

```ts
	async function toggleSidebarView(): Promise<void> {
		const activeId = entries[activeIndex]?.id;
		sidebarView = sidebarView === 'tree' ? 'list' : 'tree';
		if (activeId === undefined) return;
		const nextIndex = entries.findIndex((entry) => entry.id === activeId);
		if (nextIndex < 0) return;
		activeIndex = nextIndex;
		// The keyed file sections move on the next render; scroll after they have.
		await tick();
		selectEntry(nextIndex);
	}
```

- In the keydown `switch`, after `toggle-stage`:

```ts
				case 'toggle-tree':
					if (inField || source.kind !== 'git') return;
					event.preventDefault();
					void toggleSidebarView();
					break;
```

- Pass the new props: `<ChangesSidebar {entries} {stageable} {activeIndex} onSelect={selectEntry} onStage={handleStage} onUnstage={handleUnstage} view={sidebarView} onToggleView={() => void toggleSidebarView()} />`
  (the `onStageMany`/`onUnstageMany` props arrive in Task 6).

**Step 4: Run, expect pass.** Same command. Expected: every DiffView test PASS, including the
existing `] and [` and pair-mode tests.

**Step 5: Commit.**
```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/DiffView.svelte src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/DiffView.test.ts
git commit -m "feat: diff view opens in tree view and t toggles the sidebar and pane order"
```

---

### Task 6: sequential folder staging in `DiffView.svelte`

**Files:**
- Modify: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/DiffView.svelte`
- Test: `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/DiffView.test.ts`

**Acceptance Criteria:**
1. `handleStage` and `handleUnstage` return `Promise<boolean>`: `true` on success, `false` after
   setting `stagingError`.
2. `handleStageMany(entries)` stages each entry in order, awaiting each, and stops at the first
   `false`; `handleUnstageMany` likewise.
3. Ticking a folder of three files sends three `file-viewer.stage` calls in tree order; when the
   second rejects, the third is never sent and `.banner.error` shows the message.
4. Existing stage and unstage tests pass.

**Step 1: Write the failing test** (append inside `describe('DiffView', ...)`):

```ts
	test('a folder checkbox stages its files one by one in tree order and stops at the first failure', async () => {
		const file = (path: string, letter: string): ChangedFile => ({
			path, language: 'typescript', base: side(`old ${letter}\n`, `${letter}-base`), index: side(`old ${letter}\n`, `${letter}-base`), target: side(`new ${letter}\n`, `${letter}-target`), untracked: false,
		});
		const changeSet = changeSetFixture({ files: [file('lib/c.ts', 'c'), file('lib/a.ts', 'a'), file('lib/b.ts', 'b')] });
		mockChangesAndFingerprint(changeSet);
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelectorAll('.diff-file')).toHaveLength(3));

		const stagedPaths: string[] = [];
		callMock.mockImplementation((method: string, params: { path: string }) => {
			if (method === 'file-viewer.stage') {
				stagedPaths.push(params.path);
				return params.path === 'lib/b.ts' ? Promise.reject({ code: -32022, message: 'index.lock exists' }) : Promise.resolve({});
			}
			return Promise.resolve(undefined);
		});

		const folderButton = container.querySelector('.folder-row button[data-group="unstaged"][data-folder="lib"]');
		await fireEvent.click(folderButton!.closest('.folder-row')!.querySelector('.checkbox')!);

		await vi.waitFor(() => expect(container.querySelector('.banner.error')?.textContent).toContain('index.lock exists'));
		expect(stagedPaths).toEqual(['lib/a.ts', 'lib/b.ts']);
	});
```

**Step 2: Run, expect failure.** Expected: FAIL (nothing staged, `onStageMany` not wired).

**Step 3: Implement.**
- `handleStage`: change the signature to `async function handleStage(entry: ReviewEntry): Promise<boolean>`,
  `return true;` after `stagingError = null;`, `return false;` at the end of the `catch`, and
  `if (!changeSet) return false;` at the top.
- `handleUnstage`: the same three changes.
- Add after `handleUnstage`:

```ts
	// Folder checkbox: one file at a time in tree order, stopping at the first
	// failure so the banner shows it and nothing after it is attempted. Files
	// already staged stay staged; the sidebar shows the honest half-done state.
	async function handleStageMany(targets: ReviewEntry[]): Promise<void> {
		for (const entry of targets) {
			if (!(await handleStage(entry))) return;
		}
	}

	async function handleUnstageMany(targets: ReviewEntry[]): Promise<void> {
		for (const entry of targets) {
			if (!(await handleUnstage(entry))) return;
		}
	}
```

- `toggleActiveStage` already discards the promise with `void`; leave it.
- Add to the `<ChangesSidebar>` props: `onStageMany={(targets) => void handleStageMany(targets)} onUnstageMany={(targets) => void handleUnstageMany(targets)}`.

**Step 4: Run, expect pass.** Same command. Expected: all DiffView tests PASS.

**Step 5: Commit.**
```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/DiffView.svelte src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/DiffView.test.ts
git commit -m "feat: stage or unstage a whole folder from the diff tree"
```

---

### Task 7: whole-suite verification, build, and documentation

**Files:**
- Modify: `AGENTS.md` (the file-viewer "Diff mode" paragraph)
- Modify: `docs/plans/2026-10-13-qv-diff-tree-view-design.md` (status line)

**Acceptance Criteria:**
1. `just frontend-test` passes (every view's vitest suite).
2. The file-viewer view builds: `$RUN "cd src/ui && pnpm --filter @quantum/client build && pnpm --filter <file-viewer package name from its package.json> build"` exits 0.
3. `$RUN "cd $VIEW && pnpm exec svelte-check"` (or the view's `check` script if it has one) reports no new errors.
4. AGENTS.md's Diff mode paragraph says the sidebar opens as a folder tree (compacted, folders and
   files mixed alphabetically, pane follows it), `t` or the title button switches to the flat list,
   and a folder checkbox stages or unstages everything under it one file at a time.
5. The design doc's status line reads "implemented" with the commit range.

**Steps:**
1. Run the three commands; fix anything red before going on.
2. Edit AGENTS.md (one or two sentences inside the existing Diff mode paragraph, no new section).
3. Edit the design doc status line.
4. Commit: `git commit -m "docs: record the qv diff tree view" -- AGENTS.md docs/plans/2026-10-13-qv-diff-tree-view-design.md`

---

### Task 8: ship to x1 and walk it with Ross

The installed qv is the nix-packaged quantum from the config flake (`code/infra/config`, input
`quantum`), so the real user path is: push quantum, bump the input, build, Ross switches.

**Acceptance Criteria:**
1. quantum `main` pushed (it is a PUBLIC repository: review `git diff origin/main..HEAD` for secrets
   first; the murmur8 fixture paths are not secrets).
2. Config: `nix flake update quantum` (in `code/infra/config`), `just build` for x1 succeeds, commit
   `chore(x1): bump quantum (qv diff tree view)` and push. Follow the `nixos-config-change` skill.
3. Hand off to Ross for `just switch` (the agent never runs it).
4. After the switch, walk Ross through, one case at a time, with `qv --diff` in a repository with a
   mixed staged state: opens as tree; folders compacted; `t` and the title button switch; folding;
   `]`/`[` follow the tree; a folder checkbox in Unstaged stages its files; in Staged unstages them.
5. Update the exocortex inbox item "qv: the diff changeset should have a toggleable tree view" to done.
