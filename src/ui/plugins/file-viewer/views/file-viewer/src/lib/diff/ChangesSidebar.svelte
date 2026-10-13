<script lang="ts">
	import { splitContentLines } from './lineDiff';
	// The 260px "Changes" sidebar for diff mode: lists every `ReviewEntry`,
	// grouped into Unstaged/Staged when the change set is stageable, with a
	// checkbox per entry that stages/unstages without selecting it. With
	// `view: 'tree'` each group shows its files as a folder tree (foldable
	// folders, a folder checkbox that stages everything below it); the
	// default `'list'` is the flat list. A
	// `ReviewEntry` carries no additions/deletions counts (reviewModel.ts
	// only carries blobs and sides), so this component diffs each entry's
	// old/new content itself, the same way `DiffFile` does, purely to show
	// the "+N -M" mini stat.
	import type { ReviewEntry } from './reviewModel';
	import { cachedLineDiff } from './lineDiffCache';
	import {
		buildChangeTree,
		entriesUnder,
		flattenChangeTree,
		type FolderNode,
		type SidebarView,
		type TreeRow,
	} from './changeTree';

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

	let {
		entries,
		stageable,
		activeIndex,
		onSelect,
		onStage,
		onUnstage,
		view = 'list',
		onToggleView,
		onStageMany,
		onUnstageMany,
	}: Props = $props();

	interface IndexedEntry {
		entry: ReviewEntry;
		index: number;
		additions: number;
		deletions: number;
	}

	function countChanges(entry: ReviewEntry): { additions: number; deletions: number } {
		const oldLines = splitContentLines(entry.oldSide?.content);
		const newLines = splitContentLines(entry.newSide?.content);
		const items = cachedLineDiff(entry.oldSide?.blob, entry.newSide?.blob, oldLines, newLines);
		let additions = 0;
		let deletions = 0;
		for (const item of items) {
			if (item.type === 'change') {
				additions += item.added.length;
				deletions += item.removed.length;
			}
		}
		return { additions, deletions };
	}

	let indexedEntries = $derived(
		entries.map((entry, index): IndexedEntry => ({ entry, index, ...countChanges(entry) })),
	);
	let unstagedEntries = $derived(indexedEntries.filter((item) => item.entry.section === 'unstaged'));
	let stagedEntries = $derived(indexedEntries.filter((item) => item.entry.section === 'staged'));

	type SidebarGroup = 'unstaged' | 'staged';
	// Sidebar groups the reader has folded away. Purely a navigation aid: the
	// files themselves stay in the main pane.
	let collapsedGroups: Set<SidebarGroup> = $state(new Set());

	function toggleGroup(group: SidebarGroup): void {
		const next = new Set(collapsedGroups);
		if (next.has(group)) {
			next.delete(group);
		} else {
			next.add(group);
		}
		collapsedGroups = next;
	}
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
		// preventDefault keeps the native checked state from flipping: the
		// visual is class-driven and the model only changes when the parent
		// re-renders with new entries, so the DOM must not drift ahead of it.
		event.preventDefault();
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

	let fileCount = $derived(new Set(entries.map((entry) => entry.fileId)).size);

	function splitPath(path: string): { directory: string; name: string } {
		const index = path.lastIndexOf('/');
		return index >= 0 ? { directory: path.slice(0, index + 1), name: path.slice(index + 1) } : { directory: '', name: path };
	}

	function handleCheckboxClick(event: MouseEvent, entry: ReviewEntry): void {
		event.stopPropagation();
		if (entry.section === 'staged') {
			onUnstage?.(entry);
		} else {
			onStage?.(entry);
		}
	}

	function handleCheckboxKeydown(event: KeyboardEvent, entry: ReviewEntry): void {
		if (event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			event.stopPropagation();
			if (entry.section === 'staged') {
				onUnstage?.(entry);
			} else {
				onStage?.(entry);
			}
		}
	}
</script>

{#snippet groupHeading(group: SidebarGroup, label: string, count: number)}
	<li class="group-heading">
		<button
			type="button"
			data-group={group}
			aria-expanded={!collapsedGroups.has(group)}
			onclick={() => toggleGroup(group)}
		>
			<span class="group-label"><span class="chevron">{collapsedGroups.has(group) ? '\u25B6' : '\u25BC'}</span>{label}</span>
			<span>{count}</span>
		</button>
	</li>
{/snippet}

{#snippet entryRow(item: IndexedEntry, depth: number | null = null)}
	{@const current = splitPath(item.entry.path)}
	<li class="entry" class:tree={depth !== null} class:stageable={depth !== null && stageable} class:active={item.index === activeIndex}>
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
		<button
			type="button"
			style={depth !== null ? `padding-left: ${depth * 16}px` : undefined}
			onclick={() => onSelect(item.index)}
		>
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
		class:stageable
		class:active={!row.expanded && activeEntryId !== undefined && entriesUnder(row.folder).some((entry) => entry.id === activeEntryId)}
	>
		{#if stageable}
			<input
				type="checkbox"
				class="checkbox"
				class:checked={group === 'staged'}
				checked={group === 'staged'}
				aria-label={`${group === 'staged' ? 'Unstage' : 'Stage'} everything in ${row.folder.path}`}
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
			<span class="chevron" aria-hidden="true">{row.expanded ? '\u25BC' : '\u25B6'}</span>
			<span class="folder-name"><bdi>{row.folder.name}/</bdi></span>
		</button>
	</li>
{/snippet}

{#snippet groupBody(group: TreeGroup, items: IndexedEntry[])}
	{#if view === 'tree'}
		{#each treeRows(group, items) as row (row.kind === 'folder' ? `folder|${row.folder.path}` : `file|${row.entry.id}`)}
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

<nav class="changes-sidebar">
	<div class="sidebar-title">
		<span>Changes</span>
		<span class="count">{fileCount} files</span>
		{#if onToggleView}
			<button
				type="button"
				class="view-toggle"
				aria-label="Tree view"
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

<style>
	.changes-sidebar {
		width: 260px;
		flex-shrink: 0;
		border-right: 1px solid var(--color-border);
		display: flex;
		flex-direction: column;
		overflow: hidden;
		font-family: system-ui, sans-serif;
	}

	.sidebar-title {
		padding: 12px;
		font-size: 13px;
		font-weight: 600;
		border-bottom: 1px solid var(--color-border);
		display: flex;
		align-items: center;
		gap: 6px;
		color: var(--color-fg);
	}

	.sidebar-title .count {
		margin-left: auto;
		font-weight: 400;
		color: var(--color-muted);
		font-size: 12px;
	}

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

	.file-list {
		list-style: none;
		overflow-y: auto;
		flex: 1;
		margin: 0;
		padding: 0;
	}

	.group-heading button {
		width: 100%;
		border: none;
		background: none;
		cursor: pointer;
		padding: 10px 12px 4px;
		font-family: inherit;
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		color: var(--color-muted);
		display: flex;
		justify-content: space-between;
	}

	.group-heading button:hover {
		color: var(--color-fg);
	}

	.group-label {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.chevron {
		font-size: 8px;
	}

	.entry {
		display: grid;
		grid-template-columns: auto 1fr;
		align-items: center;
		gap: 6px;
		padding: 0 12px;
	}

	.entry button {
		width: 100%;
		min-width: 0;
		border: none;
		background: none;
		text-align: left;
		padding: 7px 0;
		cursor: pointer;
		display: grid;
		grid-template-columns: 14px 1fr auto;
		gap: 6px;
		align-items: baseline;
		color: var(--color-fg-alt);
		font-size: 13px;
	}

	.entry button:hover {
		background: var(--color-surface);
	}

	.entry.active button {
		color: var(--color-accent);
		font-weight: 600;
	}

	/* Tree rows are one line: the inline padding-left carries the depth indent,
	   so only the vertical padding is set here. List rows keep 7px 0. */
	.entry.tree button {
		padding-top: 4px;
		padding-bottom: 4px;
	}

	/* Without a checkbox the button is the only grid item: it must fill the
	   row (minmax(0, 1fr)) so long folder names truncate instead of sizing
	   the column to their full width. */
	.folder-row {
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		align-items: center;
		gap: 6px;
		padding: 0 12px;
	}

	.folder-row.stageable {
		grid-template-columns: auto minmax(0, 1fr);
	}

	.entry.tree {
		grid-template-columns: minmax(0, 1fr);
	}

	.entry.tree.stageable {
		grid-template-columns: auto minmax(0, 1fr);
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

	.checkbox {
		appearance: none;
		-webkit-appearance: none;
		margin: 0;
		width: 14px;
		height: 14px;
		border: 1px solid var(--color-muted);
		border-radius: 3px;
		cursor: pointer;
	}

	.checkbox.checked {
		background: var(--color-accent);
		border-color: var(--color-accent);
	}

	.status {
		font-family: var(--font-mono);
		font-size: 11px;
		font-weight: 700;
	}

	.status-M {
		color: #ffcb6b;
	}

	.status-A {
		color: var(--color-accent);
	}

	.status-D {
		color: var(--color-error, #e5484d);
	}

	.status-U {
		color: #89ddff;
	}

	.status-R {
		color: #82aaff;
	}

	.filename {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.directory {
		display: block;
		font-size: 11px;
		color: var(--color-muted);
		font-weight: 400;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		direction: rtl;
		text-align: left;
	}

	/* "renamed from <old name>" is an ordinary left-to-right English label,
	   not a path needing the rtl-ellipsis trick above — same specificity,
	   declared after `.directory` so it wins the cascade. */
	.rename {
		direction: ltr;
	}

	.dot {
		display: inline-block;
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: #ffcb6b;
		vertical-align: middle;
	}

	.name-block {
		min-width: 0;
	}

	.name-line {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
	}

	.mini-stats {
		white-space: nowrap;
		font-family: var(--font-mono);
		font-size: 11px;
		font-weight: 400;
		color: var(--color-muted);
	}
</style>
