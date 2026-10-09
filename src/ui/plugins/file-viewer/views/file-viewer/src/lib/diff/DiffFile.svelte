<script lang="ts">
	import { splitContentLines } from './lineDiff';
	// One file's sticky header plus its rows, for diff mode. Computes the
	// line diff and whole-file highlighting itself from `entry.oldSide` /
	// `entry.newSide` content, so a caller only ever hands this component a
	// `ReviewEntry` — never IPC, never line arrays. Placeholders stand in for
	// binary, oversized, and very large diffs; `DiffRows` renders everything
	// else.
	import type { ReviewEntry } from './reviewModel';
	import { buildRows } from './rows';
	import { cachedLineDiff } from './lineDiffCache';
	import { cachedHighlightLines } from './highlightCache';
	import { escapeHtml } from '../highlighter';
	import DiffRows, { type SearchRanges } from './DiffRows.svelte';

	const LARGE_DIFF_THRESHOLD = 1000;

	interface Props {
		entry: ReviewEntry;
		stageable: boolean;
		layout: 'unified' | 'split';
		collapsed?: boolean;
		onToggleCollapsed?: () => void;
		onStage?: (entry: ReviewEntry) => void;
		onUnstage?: (entry: ReviewEntry) => void;
		searchRanges?: SearchRanges;
		/**
		 * Optional external control of which collapsed-region keys are
		 * expanded, mirroring the `collapsed` prop's own pattern: when
		 * provided, this is the source of truth and `onExpandedKeysChange`
		 * reports every toggle instead of this component tracking its own
		 * state. Lets `DiffView` force-expand the region around a search
		 * match before scrolling to it. Omitted (the default) preserves the
		 * original uncontrolled behavior exactly.
		 */
		expandedKeys?: ReadonlySet<string>;
		onExpandedKeysChange?: (keys: Set<string>) => void;
	}

	let {
		entry,
		stageable,
		layout,
		collapsed = entry.section === 'staged',
		onToggleCollapsed,
		onStage,
		onUnstage,
		searchRanges,
		expandedKeys,
		onExpandedKeysChange,
	}: Props = $props();

	let isBinary = $derived(!!entry.oldSide?.binary || !!entry.newSide?.binary);
	let isTooLarge = $derived(!!entry.oldSide?.too_large || !!entry.newSide?.too_large);

	let oldLines = $derived(splitContentLines(entry.oldSide?.content));
	let newLines = $derived(splitContentLines(entry.newSide?.content));

	let diffItems = $derived(cachedLineDiff(entry.oldSide?.blob, entry.newSide?.blob, oldLines, newLines));
	let additions = $derived(diffItems.reduce((sum, item) => (item.type === 'change' ? sum + item.added.length : sum), 0));
	let deletions = $derived(diffItems.reduce((sum, item) => (item.type === 'change' ? sum + item.removed.length : sum), 0));
	let changedLineCount = $derived(additions + deletions);
	let isLargeDiff = $derived(changedLineCount > LARGE_DIFF_THRESHOLD);

	let loadedLarge = $state(false);
	let internalExpandedKeys: Set<string> = $state(new Set());
	let effectiveExpandedKeys = $derived(expandedKeys ?? internalExpandedKeys);

	$effect(() => {
		// Reset per-file UI state (collapsed-region expansion, the large-diff
		// load gate) whenever the entry identity changes, so switching files
		// in a stacked view never leaks another file's expanded regions. Only
		// resets the INTERNAL set — a caller controlling `expandedKeys`
		// explicitly owns resetting it on entry change itself.
		void entry.id;
		internalExpandedKeys = new Set();
		loadedLarge = false;
	});

	let rows = $derived(buildRows(diffItems, oldLines, newLines, entry.language, effectiveExpandedKeys));
	let oldTokens = $derived(cachedHighlightLines(entry.oldSide?.content ?? '', entry.language, entry.oldSide?.blob));
	let newTokens = $derived(cachedHighlightLines(entry.newSide?.content ?? '', entry.language, entry.newSide?.blob));

	function handleExpand(key: string): void {
		const next = new Set(effectiveExpandedKeys);
		next.add(key);
		if (onExpandedKeysChange) {
			onExpandedKeysChange(next);
		} else {
			internalExpandedKeys = next;
		}
	}

	function handleRecollapse(key: string): void {
		const next = new Set(effectiveExpandedKeys);
		next.delete(key);
		if (onExpandedKeysChange) {
			onExpandedKeysChange(next);
		} else {
			internalExpandedKeys = next;
		}
	}

	function handleHeaderClick(): void {
		onToggleCollapsed?.();
	}

	function handleHeaderKeydown(event: KeyboardEvent): void {
		if (event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			onToggleCollapsed?.();
		}
	}

	function handleStageClick(event: MouseEvent): void {
		event.stopPropagation();
		if (entry.section === 'staged') {
			onUnstage?.(entry);
		} else {
			onStage?.(entry);
		}
	}

	function handleLoadLarge(event: MouseEvent): void {
		event.stopPropagation();
		loadedLarge = true;
	}

	interface SplitPath {
		directory: string;
		name: string;
	}

	function splitPath(path: string): SplitPath {
		const index = path.lastIndexOf('/');
		return index >= 0 ? { directory: path.slice(0, index + 1), name: path.slice(index + 1) } : { directory: '', name: path };
	}

	let pathHtml = $derived.by(() => {
		const current = splitPath(entry.path);
		if (!entry.oldPath) {
			return `<span class="directory">${escapeHtml(current.directory)}</span>${escapeHtml(current.name)}`;
		}
		const old = splitPath(entry.oldPath);
		if (old.directory === current.directory) {
			return (
				`<span class="directory">${escapeHtml(current.directory)}</span>` +
				`<span class="directory">${escapeHtml(old.name)} \u2192 </span>${escapeHtml(current.name)}`
			);
		}
		return (
			`<span class="directory">${escapeHtml(old.directory)}</span>${escapeHtml(old.name)} \u2192 ` +
			`<span class="directory">${escapeHtml(current.directory)}</span>${escapeHtml(current.name)}`
		);
	});
</script>

<section class="diff-file">
	<div class="file-header" role="button" tabindex="0" onclick={handleHeaderClick} onkeydown={handleHeaderKeydown}>
		<span class="chevron">{collapsed ? '\u25B6' : '\u25BC'}</span>
		<span class="status status-{entry.status}">{entry.status}</span>
		<span class="path">{@html pathHtml}</span>
		{#if entry.partiallyStaged}<span class="pill">partially staged</span>{/if}
		<span class="stats"><span class="additions">+{additions}</span> <span class="deletions">&#x2212;{deletions}</span></span>
		{#if stageable}
			<button type="button" class="stage-button" class:staged={entry.section === 'staged'} onclick={handleStageClick}>
				{entry.section === 'staged' ? '\u2713 Staged' : 'Stage'}
			</button>
		{/if}
	</div>
	{#if !collapsed}
		<div class="file-body">
			{#if isBinary}
				<div class="placeholder">Binary file changed</div>
			{:else if isTooLarge}
				<div class="placeholder">Too large to diff</div>
			{:else if isLargeDiff && !loadedLarge}
				<div class="large-diff">
					<span>Large diff: {changedLineCount.toLocaleString()} changed lines, not rendered yet.</span>
					<button type="button" onclick={handleLoadLarge}>Load diff</button>
				</div>
			{:else}
				<DiffRows {rows} {layout} {oldTokens} {newTokens} {searchRanges} onExpand={handleExpand} onRecollapse={handleRecollapse} />
			{/if}
		</div>
	{/if}
</section>

<style>
	.diff-file {
		position: relative;
	}

	.file-header {
		position: sticky;
		top: 0;
		z-index: 2;
		display: flex;
		align-items: center;
		gap: 8px;
		height: 32px;
		padding: 0 12px;
		background: var(--color-bg-alt);
		border-bottom: 1px solid var(--color-border);
		font-family: var(--font-mono);
		font-size: 12px;
		cursor: pointer;
	}

	.chevron {
		color: var(--color-muted);
		font-size: 9px;
		width: 10px;
		flex-shrink: 0;
	}

	.status {
		font-weight: 700;
		flex-shrink: 0;
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

	.path {
		flex: 1;
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		direction: rtl;
		text-align: left;
	}

	.path :global(.directory) {
		color: var(--color-muted);
	}

	.pill {
		font-family: system-ui, sans-serif;
		font-size: 10px;
		padding: 1px 6px;
		border-radius: 8px;
		border: 1px solid var(--color-border);
		color: var(--color-muted);
		white-space: nowrap;
		flex-shrink: 0;
	}

	.stats {
		font-family: var(--font-mono);
		white-space: nowrap;
		color: var(--color-muted);
		flex-shrink: 0;
	}

	.additions {
		color: var(--color-accent);
	}

	.deletions {
		color: var(--color-error, #e5484d);
	}

	.stage-button {
		font-family: system-ui, sans-serif;
		font-size: 11px;
		padding: 2px 8px;
		border-radius: 4px;
		border: 1px solid var(--color-border);
		background: var(--color-bg);
		color: var(--color-fg-alt);
		cursor: pointer;
		white-space: nowrap;
		flex-shrink: 0;
	}

	.stage-button:hover {
		border-color: var(--color-accent);
		color: var(--color-accent);
	}

	.stage-button.staged {
		color: var(--color-accent);
		border-color: color-mix(in oklab, var(--color-accent) 45%, transparent);
		background: color-mix(in oklab, var(--color-accent) 10%, transparent);
	}

	.placeholder {
		margin: 8px 12px;
		padding: 10px 12px;
		color: var(--color-muted);
		font-size: 12px;
	}

	.large-diff {
		margin: 8px 12px;
		padding: 10px 12px;
		border: 1px dashed var(--color-border);
		border-radius: 6px;
		color: var(--color-muted);
		font-size: 12px;
		display: flex;
		align-items: center;
		gap: 10px;
	}

	.large-diff button {
		font-size: 11px;
		padding: 2px 8px;
		border-radius: 4px;
		border: 1px solid var(--color-border);
		background: var(--color-bg-alt);
		color: var(--color-fg);
		cursor: pointer;
	}
</style>
