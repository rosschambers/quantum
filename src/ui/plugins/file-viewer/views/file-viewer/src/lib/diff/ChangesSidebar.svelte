<script lang="ts">
	import { splitContentLines } from './lineDiff';
	// The 220px "Changes" sidebar for diff mode: lists every `ReviewEntry`,
	// grouped into Unstaged/Staged when the change set is stageable, with a
	// checkbox per entry that stages/unstages without selecting it. A
	// `ReviewEntry` carries no additions/deletions counts (reviewModel.ts
	// only carries blobs and sides), so this component diffs each entry's
	// old/new content itself, the same way `DiffFile` does, purely to show
	// the "+N -M" mini stat.
	import type { ReviewEntry } from './reviewModel';
	import { cachedLineDiff } from './lineDiffCache';

	interface Props {
		entries: ReviewEntry[];
		stageable: boolean;
		activeIndex: number;
		onSelect: (index: number) => void;
		onStage?: (entry: ReviewEntry) => void;
		onUnstage?: (entry: ReviewEntry) => void;
	}

	let { entries, stageable, activeIndex, onSelect, onStage, onUnstage }: Props = $props();

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

{#snippet entryRow(item: IndexedEntry)}
	{@const current = splitPath(item.entry.path)}
	<li class="entry" class:active={item.index === activeIndex}>
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
		<button type="button" onclick={() => onSelect(item.index)}>
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
				{:else}
					<span class="directory">{current.directory}</span>
				{/if}
			</span>
			<span class="mini-stats">+{item.additions} &#x2212;{item.deletions}</span>
		</button>
	</li>
{/snippet}

<nav class="changes-sidebar">
	<div class="sidebar-title">Changes <span class="count">{fileCount} files</span></div>
	<ul class="file-list">
		{#if stageable}
			<li class="group-heading">Unstaged <span>{unstagedEntries.length}</span></li>
			{#each unstagedEntries as item (item.entry.id)}
				{@render entryRow(item)}
			{/each}
			<li class="group-heading">Staged <span>{stagedEntries.length}</span></li>
			{#each stagedEntries as item (item.entry.id)}
				{@render entryRow(item)}
			{/each}
		{:else}
			{#each indexedEntries as item (item.entry.id)}
				{@render entryRow(item)}
			{/each}
		{/if}
	</ul>
</nav>

<style>
	.changes-sidebar {
		width: 220px;
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
		justify-content: space-between;
		color: var(--color-fg);
	}

	.sidebar-title .count {
		font-weight: 400;
		color: var(--color-muted);
		font-size: 12px;
	}

	.file-list {
		list-style: none;
		overflow-y: auto;
		flex: 1;
		margin: 0;
		padding: 0;
	}

	.group-heading {
		padding: 10px 12px 4px;
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		color: var(--color-muted);
		display: flex;
		justify-content: space-between;
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
