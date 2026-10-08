<script lang="ts" module>
	// The top-level diff-mode view: loads a `ChangeSet` (git source) or builds
	// one from two independent reads (pair source), then renders the header,
	// the live-changes banner, search, the Changes sidebar plus stacked
	// `DiffFile` sections, the shared overview ruler, and the key-hint bar —
	// everything `App.svelte` delegates to for `qv --diff` / `qv <a> <b>`
	// (design doc "Settled visual and interaction spec"). Mounted by App.svelte
	// in place of the regular file-viewer markup; owns its own search state,
	// keyboard handling, and close button rather than sharing App's, the same
	// way App itself is a self-contained top-level view.
	import type { DiffSpec } from '@quantum/client';

	export type DiffViewSource = { kind: 'git'; spec: DiffSpec } | { kind: 'pair'; left: string; right: string };
</script>

<script lang="ts">
	import { createClient } from '@quantum/client';
	import type { ChangeSet, FingerprintResult } from '@quantum/client';
	import type { ViewerFileInfo } from '../types';
	import { selfViewName } from '../selfName';
	import { resolveViewerShortcut } from '../viewerKeymap';
	import { searchMarks, type RulerMark, type RulerMarkKind } from '../overviewRuler';
	import { findMatchesInLines } from '../search';
	import SearchBar from '../SearchBar.svelte';
	import OverviewRuler from '../OverviewRuler.svelte';
	import ChangesSidebar from './ChangesSidebar.svelte';
	import DiffFile from './DiffFile.svelte';
	import type { SearchRanges, LineSearchRanges } from './DiffRows.svelte';
	import { reviewEntries, stagingProgress, applyLocalStage, applyLocalUnstage, type ReviewEntry } from './reviewModel';
	import { cachedLineDiff } from './lineDiffCache';
	import { buildRows } from './rows';
	import { pairChangeSet } from './pairChangeSet';

	interface Props {
		source: DiffViewSource;
	}

	let { source }: Props = $props();

	const client = createClient();

	/**
	 * A fake "expanded" set whose `has()` always returns true. Passed to
	 * `buildRows` so it returns every line as its own `context`/`removed`/
	 * `added` row — no `collapsed` rows at all — purely as a way to walk a
	 * file's full row list (for search and the change-marks ruler) without
	 * caring about any file's actual current collapse state.
	 */
	class AlwaysExpandedKeys extends Set<string> {
		override has(): boolean {
			return true;
		}
	}
	const ALWAYS_EXPANDED: ReadonlySet<string> = new AlwaysExpandedKeys();

	let isLoading = $state(true);
	let error: string | null = $state(null);
	let changeSet: ChangeSet | null = $state(null);
	let layout: 'unified' | 'split' = $state('unified');
	let activeIndex = $state(0);
	let collapsedOverrides: Map<string, boolean> = $state(new Map());
	let expandedKeysByEntry: Map<string, Set<string>> = $state(new Map());
	let stagingError: string | null = $state(null);
	let diskChanged = $state(false);
	let baselineFingerprint: string | null = $state(null);
	let repositoryRootForPolling: string | null = $state(null);

	let searchOpen = $state(false);
	let searchQuery = $state('');
	let searchFocusSignal = $state(0);
	let totalMatches = $state(0);
	let currentMatchIndex: number | null = $state(null);
	let navigationRevision = $state(0);
	let activeQuery = $derived(searchOpen ? searchQuery : '');

	let paneElement: HTMLDivElement | undefined = $state(undefined);
	let fileSectionElements: (HTMLDivElement | undefined)[] = $state([]);

	function errorMessage(candidate: unknown): string {
		if (candidate && typeof candidate === 'object' && 'message' in candidate) {
			const message = (candidate as { message: unknown }).message;
			if (typeof message === 'string' && message.trim() !== '') {
				return message;
			}
		}
		if (candidate instanceof Error) {
			return candidate.message;
		}
		if (typeof candidate === 'string' && candidate.trim() !== '') {
			return candidate;
		}
		return 'Request failed';
	}

	function isWorkingTreeTarget(spec: DiffSpec): boolean {
		return spec.target === undefined || spec.target === null;
	}

	async function loadPair(left: string, right: string): Promise<ChangeSet> {
		const [leftInfo, rightInfo] = await Promise.all([
			client.call('file-viewer.read', { path: left }) as Promise<ViewerFileInfo>,
			client.call('file-viewer.read', { path: right }) as Promise<ViewerFileInfo>,
		]);
		return pairChangeSet(
			{
				path: left,
				content: leftInfo.content,
				language: leftInfo.language,
				binary: leftInfo.file_type === 'image' || leftInfo.file_type === 'video',
			},
			{
				path: right,
				content: rightInfo.content,
				language: rightInfo.language,
				binary: rightInfo.file_type === 'image' || rightInfo.file_type === 'video',
			},
		);
	}

	// Initial load. Mirrors App.svelte's own cancelled-flag pattern: the
	// effect body stays synchronous, the async work runs in a fire-and-forget
	// inner function.
	$effect(() => {
		let cancelled = false;
		isLoading = true;
		error = null;
		changeSet = null;

		void (async () => {
			try {
				let result: ChangeSet;
				if (source.kind === 'git') {
					result = (await client.call('file-viewer.changes', source.spec)) as ChangeSet;
				} else {
					result = await loadPair(source.left, source.right);
				}
				if (cancelled) return;
				changeSet = result;
				repositoryRootForPolling = result.repository_root;
				isLoading = false;

				if (source.kind === 'git' && isWorkingTreeTarget(source.spec)) {
					try {
						const fingerprintResult = (await client.call('file-viewer.fingerprint', {
							repository_root: result.repository_root,
						})) as FingerprintResult;
						if (!cancelled) {
							baselineFingerprint = fingerprintResult.fingerprint;
						}
					} catch {
						// Best effort: a failed baseline read only means the
						// disk-changed banner never fires for this session —
						// it never surfaces as a load error.
					}
				}
			} catch (loadError) {
				if (cancelled) return;
				error = errorMessage(loadError);
				isLoading = false;
			}
		})();

		return () => {
			cancelled = true;
		};
	});

	// Live-changes polling (design doc "Live changes"): only while the
	// target is the working tree. Depends only on the STABLE repository
	// root captured at load, never on `changeSet` itself, so staging a file
	// (which replaces `changeSet` with a new object) never restarts the
	// timer.
	$effect(() => {
		if (source.kind !== 'git' || !isWorkingTreeTarget(source.spec) || repositoryRootForPolling === null) {
			return;
		}
		const root = repositoryRootForPolling;
		const interval = setInterval(() => {
			void (async () => {
				try {
					const result = (await client.call('file-viewer.fingerprint', { repository_root: root })) as FingerprintResult;
					if (baselineFingerprint !== null && result.fingerprint !== baselineFingerprint) {
						diskChanged = true;
					}
				} catch {
					// Best effort: a transient fingerprint failure only skips
					// staleness detection for this tick.
				}
			})();
		}, 2000);
		return () => clearInterval(interval);
	});

	async function refresh(): Promise<void> {
		if (source.kind !== 'git') {
			return;
		}
		try {
			const result = (await client.call('file-viewer.changes', source.spec)) as ChangeSet;
			changeSet = result;
			diskChanged = false;
			stagingError = null;
			try {
				const fingerprintResult = (await client.call('file-viewer.fingerprint', {
					repository_root: result.repository_root,
				})) as FingerprintResult;
				baselineFingerprint = fingerprintResult.fingerprint;
			} catch {
				// Best effort, as at initial load.
			}
		} catch (refreshError) {
			stagingError = errorMessage(refreshError);
		}
	}

	async function handleStage(entry: ReviewEntry): Promise<void> {
		if (!changeSet) return;
		try {
			await client.call('file-viewer.stage', {
				repository_root: changeSet.repository_root,
				path: entry.path,
				blob: entry.newSide?.blob ?? null,
				mode: entry.newSide?.mode ?? entry.oldSide?.mode ?? '100644',
			});
			changeSet = applyLocalStage(changeSet, entry.path);
			stagingError = null;
		} catch (stageError) {
			stagingError = errorMessage(stageError);
		}
	}

	async function handleUnstage(entry: ReviewEntry): Promise<void> {
		if (!changeSet) return;
		try {
			await client.call('file-viewer.unstage', {
				repository_root: changeSet.repository_root,
				path: entry.path,
				old_path: entry.oldPath,
			});
			const updated = applyLocalUnstage(changeSet, entry.path);
			if (updated === null) {
				// A staged rename: the local mirror cannot represent it (see
				// applyLocalUnstage's doc), so this is the one case that
				// refetches.
				await refresh();
			} else {
				changeSet = updated;
			}
			stagingError = null;
		} catch (unstageError) {
			stagingError = errorMessage(unstageError);
		}
	}

	let entries = $derived(changeSet ? reviewEntries(changeSet) : []);
	let stageable = $derived(changeSet?.stageable ?? false);
	let progress = $derived(stagingProgress(entries));

	let totals = $derived.by(() => {
		let additions = 0;
		let deletions = 0;
		for (const entry of entries) {
			const oldLines = entry.oldSide?.content !== undefined ? entry.oldSide.content.split('\n') : [];
			const newLines = entry.newSide?.content !== undefined ? entry.newSide.content.split('\n') : [];
			const items = cachedLineDiff(entry.oldSide?.blob, entry.newSide?.blob, oldLines, newLines);
			for (const item of items) {
				if (item.type === 'change') {
					additions += item.added.length;
					deletions += item.removed.length;
				}
			}
		}
		return { additions, deletions };
	});

	function basename(path: string): string {
		const index = path.lastIndexOf('/');
		return index >= 0 ? path.slice(index + 1) : path;
	}

	let headerTitle = $derived.by(() => {
		if (!changeSet) return '';
		if (source.kind === 'pair') {
			return `${basename(changeSet.base_label)} \u2194 ${basename(changeSet.target_label)}`;
		}
		return `${changeSet.base_label} \u2192 ${changeSet.target_label}`;
	});

	let headerSubtitle = $derived.by(() => {
		if (!changeSet) return '';
		if (source.kind === 'pair') {
			return `${changeSet.base_label} \u2194 ${changeSet.target_label}`;
		}
		return changeSet.repository_root;
	});

	function isCollapsed(entry: ReviewEntry): boolean {
		return collapsedOverrides.has(entry.id) ? (collapsedOverrides.get(entry.id) as boolean) : entry.section === 'staged';
	}

	function setCollapsed(id: string, value: boolean): void {
		collapsedOverrides = new Map(collapsedOverrides).set(id, value);
	}

	function expandedKeysFor(id: string): Set<string> {
		return expandedKeysByEntry.get(id) ?? new Set();
	}

	// ================= search =================
	// Matches are computed per file over each row's displayed text: NEW-side
	// text for context and added rows, OLD-side text for removed rows (a
	// context row's content is identical on both sides, so counting it only
	// once via the new side avoids double-reporting the same text as two
	// matches). Every row is considered, including rows that would currently
	// be hidden inside a collapsed region or a collapsed (staged) file — the
	// file's FULL row list is walked with `ALWAYS_EXPANDED`, independent of
	// what is actually rendered — so search counts never miss a hidden match.
	interface RowTextRef {
		side: 'old' | 'new';
		index: number;
	}

	function buildSearchRows(entry: ReviewEntry): { texts: string[]; refs: RowTextRef[] } {
		if (entry.oldSide?.content === undefined && entry.newSide?.content === undefined) {
			return { texts: [], refs: [] };
		}
		const oldLines = entry.oldSide?.content !== undefined ? entry.oldSide.content.split('\n') : [];
		const newLines = entry.newSide?.content !== undefined ? entry.newSide.content.split('\n') : [];
		const diffItems = cachedLineDiff(entry.oldSide?.blob, entry.newSide?.blob, oldLines, newLines);
		const rows = buildRows(diffItems, oldLines, newLines, entry.language, ALWAYS_EXPANDED);
		const texts: string[] = [];
		const refs: RowTextRef[] = [];
		for (const row of rows) {
			if (row.kind === 'context' || row.kind === 'added') {
				texts.push(newLines[row.newIndex]);
				refs.push({ side: 'new', index: row.newIndex });
			} else if (row.kind === 'removed') {
				texts.push(oldLines[row.oldIndex]);
				refs.push({ side: 'old', index: row.oldIndex });
			}
		}
		return { texts, refs };
	}

	interface GlobalMatch {
		entryIndex: number;
		side: 'old' | 'new';
		lineIndex: number;
		/** Position within the entry's own full row list — used only for the ruler's approximate placement. */
		rowPosition: number;
		rowCount: number;
		range: { start: number; end: number };
	}

	let globalMatches = $derived.by(() => {
		if (!activeQuery) return [] as GlobalMatch[];
		const matches: GlobalMatch[] = [];
		entries.forEach((entry, entryIndex) => {
			const { texts, refs } = buildSearchRows(entry);
			for (const lineMatch of findMatchesInLines(texts, activeQuery)) {
				const ref = refs[lineMatch.lineIndex];
				matches.push({
					entryIndex,
					side: ref.side,
					lineIndex: ref.index,
					rowPosition: lineMatch.lineIndex,
					rowCount: texts.length,
					range: lineMatch.range,
				});
			}
		});
		return matches;
	});

	$effect(() => {
		const count = globalMatches.length;
		totalMatches = count;
		currentMatchIndex = count > 0 ? 0 : null;
	});

	let searchRangesByEntry = $derived.by(() => {
		const map = new Map<string, { old: Map<number, { ranges: [number, number][]; currentRange: [number, number] | null }>; new: Map<number, { ranges: [number, number][]; currentRange: [number, number] | null }> }>();
		globalMatches.forEach((match, matchIndex) => {
			const entry = entries[match.entryIndex];
			if (!entry) return;
			let ranges = map.get(entry.id);
			if (!ranges) {
				ranges = { old: new Map(), new: new Map() };
				map.set(entry.id, ranges);
			}
			const bucket = match.side === 'old' ? ranges.old : ranges.new;
			let lineRanges = bucket.get(match.lineIndex);
			if (!lineRanges) {
				lineRanges = { ranges: [], currentRange: null };
				bucket.set(match.lineIndex, lineRanges);
			}
			lineRanges.ranges.push([match.range.start, match.range.end]);
			if (matchIndex === currentMatchIndex) {
				lineRanges.currentRange = [match.range.start, match.range.end];
			}
		});
		return map as Map<string, SearchRanges>;
	});

	function searchRangesFor(entryId: string): SearchRanges | undefined {
		return searchRangesByEntry.get(entryId);
	}

	let matchPositions = $derived.by(() => {
		if (entries.length === 0) return new Float64Array(0);
		const positions = new Float64Array(globalMatches.length);
		globalMatches.forEach((match, index) => {
			const within = match.rowCount > 0 ? (match.rowPosition + 0.5) / match.rowCount : 0.5;
			positions[index] = (match.entryIndex + within) / entries.length;
		});
		return positions;
	});

	// ================= change marks (overview ruler) =================
	// One mark per contiguous run of added/removed rows (a replaced block's
	// removed rows are immediately followed by its added rows with no
	// context row between them — see rows.ts — so a single run naturally
	// spans both, hence 'mixed'). Positions are the same coarse
	// (entryIndex + rowPosition / rowCount) / entries.length arithmetic as
	// the search marks above, rather than a DOM measurement pass: every
	// file's rows are walked fully expanded regardless of what is actually
	// rendered, so there is no guarantee a DOM element exists yet for every
	// run to measure.
	function computeChangeMarks(reviewEntries: readonly ReviewEntry[]): RulerMark[] {
		const marks: RulerMark[] = [];
		if (reviewEntries.length === 0) return marks;
		reviewEntries.forEach((entry, entryIndex) => {
			if (entry.oldSide?.binary || entry.newSide?.binary || entry.oldSide?.too_large || entry.newSide?.too_large) {
				return;
			}
			const oldLines = entry.oldSide?.content !== undefined ? entry.oldSide.content.split('\n') : [];
			const newLines = entry.newSide?.content !== undefined ? entry.newSide.content.split('\n') : [];
			const diffItems = cachedLineDiff(entry.oldSide?.blob, entry.newSide?.blob, oldLines, newLines);
			const rows = buildRows(diffItems, oldLines, newLines, entry.language, ALWAYS_EXPANDED).filter((row) => row.kind !== 'recollapse');
			const rowCount = rows.length;
			if (rowCount === 0) return;

			let runStart: number | null = null;
			let hasAdded = false;
			let hasRemoved = false;
			const flush = (endRow: number): void => {
				if (runStart === null) return;
				const kind: RulerMarkKind = hasAdded && hasRemoved ? 'mixed' : hasAdded ? 'added' : 'removed';
				const start = (entryIndex + runStart / rowCount) / reviewEntries.length;
				const extent = (endRow - runStart) / rowCount / reviewEntries.length;
				marks.push({ start, extent, kind });
				runStart = null;
				hasAdded = false;
				hasRemoved = false;
			};
			rows.forEach((row, rowIndex) => {
				if (row.kind === 'added' || row.kind === 'removed') {
					if (runStart === null) runStart = rowIndex;
					if (row.kind === 'added') hasAdded = true;
					else hasRemoved = true;
				} else {
					flush(rowIndex);
				}
			});
			flush(rowCount);
		});
		return marks;
	}

	let changeMarks = $derived(computeChangeMarks(entries));
	let searchRulerMarks = $derived(searchMarks(matchPositions, totalMatches, currentMatchIndex));
	let rulerMarks = $derived<RulerMark[]>([...changeMarks, ...searchRulerMarks]);

	function handleMarkActivate(mark: RulerMark): void {
		if (mark.index === undefined) return;
		currentMatchIndex = mark.index;
		navigationRevision++;
	}

	/** Ensures the region (and file) containing `match` is visible before scrolling to it. Only context/added (new-side) lines can ever be hidden inside a collapsed region — removed lines never are, since only unchanged runs collapse. */
	function ensureMatchVisible(match: GlobalMatch): void {
		const entry = entries[match.entryIndex];
		if (!entry) return;
		if (isCollapsed(entry)) {
			setCollapsed(entry.id, false);
		}
		if (match.side !== 'new') return;

		const oldLines = entry.oldSide?.content !== undefined ? entry.oldSide.content.split('\n') : [];
		const newLines = entry.newSide?.content !== undefined ? entry.newSide.content.split('\n') : [];
		const diffItems = cachedLineDiff(entry.oldSide?.blob, entry.newSide?.blob, oldLines, newLines);
		const currentlyExpanded = expandedKeysFor(entry.id);
		const rows = buildRows(diffItems, oldLines, newLines, entry.language, currentlyExpanded);
		for (const row of rows) {
			if (row.kind === 'collapsed' && match.lineIndex >= row.firstNewIndex && match.lineIndex < row.firstNewIndex + row.hiddenCount) {
				const next = new Set(currentlyExpanded);
				next.add(row.key);
				expandedKeysByEntry = new Map(expandedKeysByEntry).set(entry.id, next);
				break;
			}
		}
	}

	$effect(() => {
		void navigationRevision;
		const match = currentMatchIndex !== null ? globalMatches[currentMatchIndex] : undefined;
		if (!match) return;
		ensureMatchVisible(match);
		const entryIndex = match.entryIndex;
		queueMicrotask(() => {
			fileSectionElements[entryIndex]?.scrollIntoView({ block: 'center' });
		});
	});

	function openSearch(): void {
		searchOpen = true;
		searchFocusSignal++;
	}

	function closeSearch(): void {
		searchOpen = false;
		searchQuery = '';
		currentMatchIndex = null;
	}

	function nextMatch(): void {
		if (totalMatches === 0) return;
		navigationRevision++;
		currentMatchIndex = currentMatchIndex === null ? 0 : (currentMatchIndex + 1) % totalMatches;
	}

	function previousMatch(): void {
		if (totalMatches === 0) return;
		navigationRevision++;
		currentMatchIndex = currentMatchIndex === null ? totalMatches - 1 : (currentMatchIndex - 1 + totalMatches) % totalMatches;
	}

	// ================= file / change navigation, staging shortcut =================
	const CHANGE_ROW_SELECTOR = '.row.added, .row.removed, .half.added, .half.removed';
	const STICKY_OFFSET = 40;

	function changeRunStarts(): HTMLElement[] {
		if (!paneElement) return [];
		const all = Array.from(paneElement.querySelectorAll<HTMLElement>(CHANGE_ROW_SELECTOR));
		return all.filter((element) => {
			const previous = element.previousElementSibling;
			return !(previous instanceof HTMLElement) || !previous.matches(CHANGE_ROW_SELECTOR);
		});
	}

	function goToNextChange(): void {
		if (!paneElement) return;
		const starts = changeRunStarts();
		const current = paneElement.scrollTop + STICKY_OFFSET;
		const target = starts.find((element) => element.offsetTop > current + 2);
		if (target) paneElement.scrollTop = target.offsetTop - STICKY_OFFSET;
	}

	function goToPreviousChange(): void {
		if (!paneElement) return;
		const starts = changeRunStarts();
		const current = paneElement.scrollTop + STICKY_OFFSET;
		const target = [...starts].reverse().find((element) => element.offsetTop < current - 2);
		if (target) paneElement.scrollTop = target.offsetTop - STICKY_OFFSET;
	}

	function selectEntry(index: number): void {
		activeIndex = index;
		const element = fileSectionElements[index];
		if (element && paneElement) {
			paneElement.scrollTop = element.offsetTop;
		}
	}

	function goToFile(step: number): void {
		if (entries.length === 0) return;
		selectEntry(Math.min(entries.length - 1, Math.max(0, activeIndex + step)));
	}

	function handlePaneScroll(): void {
		if (!paneElement) return;
		let active = 0;
		for (let index = 0; index < fileSectionElements.length; index++) {
			const element = fileSectionElements[index];
			if (element && element.offsetTop <= paneElement.scrollTop + 8) {
				active = index;
			}
		}
		activeIndex = active;
	}

	function toggleActiveStage(): void {
		const entry = entries[activeIndex];
		if (!entry) return;
		if (entry.section === 'staged') {
			void handleUnstage(entry);
		} else {
			void handleStage(entry);
		}
	}

	function close(): void {
		client.call('view.hide', { name: selfViewName() }).catch(console.error);
	}

	$effect(() => {
		function handleKeyDown(event: KeyboardEvent): void {
			const action = resolveViewerShortcut(event);
			if (!action) return;

			const target = event.target;

			if (action.kind === 'next-match' || action.kind === 'previous-match') {
				if (event.defaultPrevented) return;
				if (
					target instanceof Element &&
					!target.matches('.search-bar .search-input') &&
					target.closest(
						'button, a[href], input, textarea, select, summary, [contenteditable]:not([contenteditable="false"]), [role="button"], [role="link"], [role="textbox"]',
					)
				) {
					return;
				}
			}

			const inField = target instanceof Element && target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])') !== null;

			switch (action.kind) {
				case 'open-search':
					event.preventDefault();
					openSearch();
					break;
				case 'next-match':
					if (!searchOpen) return;
					event.preventDefault();
					nextMatch();
					break;
				case 'previous-match':
					if (!searchOpen) return;
					event.preventDefault();
					previousMatch();
					break;
				case 'close-search':
					if (searchOpen) {
						closeSearch();
					} else {
						close();
					}
					break;
				case 'next-change':
					if (inField) return;
					event.preventDefault();
					goToNextChange();
					break;
				case 'previous-change':
					if (inField) return;
					event.preventDefault();
					goToPreviousChange();
					break;
				case 'next-file':
					if (inField) return;
					event.preventDefault();
					goToFile(1);
					break;
				case 'previous-file':
					if (inField) return;
					event.preventDefault();
					goToFile(-1);
					break;
				case 'toggle-stage':
					if (inField || !stageable) return;
					event.preventDefault();
					toggleActiveStage();
					break;
				case 'refresh':
					if (inField || source.kind !== 'git') return;
					event.preventDefault();
					void refresh();
					break;
			}
		}

		window.addEventListener('keydown', handleKeyDown);
		return () => window.removeEventListener('keydown', handleKeyDown);
	});
</script>

<div class="diff-view">
	{#if isLoading}
		<div class="loading">
			<div class="spinner"></div>
			<p>Loading changes...</p>
		</div>
	{:else if error}
		<div class="error">
			<p class="error-title">Error</p>
			<p class="error-message">{error}</p>
		</div>
	{:else if changeSet}
		<header class="diff-header">
			<span class="badge">&#xB1;</span>
			<span class="title">{headerTitle}</span>
			<span class="dir">{headerSubtitle}</span>
			{#if stageable}
				<span class="progress" title="Files with no unstaged changes">
					<span class="bar"><span style={`width:${progress.total ? (progress.staged / progress.total) * 100 : 0}%`}></span></span>
					{progress.staged} of {progress.total} staged
				</span>
			{/if}
			<span class="stats"><span class="additions">+{totals.additions}</span> <span class="deletions">&#x2212;{totals.deletions}</span></span>
			<div class="toggle">
				<button type="button" class:on={layout === 'unified'} onclick={() => (layout = 'unified')}>Unified</button>
				<button type="button" class:on={layout === 'split'} onclick={() => (layout = 'split')}>Split</button>
			</div>
			<button type="button" class="icon-button close" title="Close" onclick={close}>&#x2715;</button>
		</header>

		{#if stagingError}
			<div class="banner error">
				{stagingError}
				<button type="button" onclick={() => (stagingError = null)}>Dismiss</button>
			</div>
		{:else if diskChanged}
			<div class="banner">
				Files changed on disk since this diff loaded.
				<button type="button" onclick={refresh}>Refresh (R)</button>
			</div>
		{/if}

		{#if searchOpen}
			<SearchBar
				query={searchQuery}
				{totalMatches}
				{currentMatchIndex}
				onQueryInput={(value) => {
					searchQuery = value;
				}}
				onNext={nextMatch}
				onPrevious={previousMatch}
				onClose={closeSearch}
				focusSignal={searchFocusSignal}
			/>
		{/if}

		{#if entries.length === 0}
			<div class="empty">
				<p>No changes</p>
			</div>
		{:else}
			<div class="body">
				{#if source.kind === 'git'}
					<ChangesSidebar {entries} {stageable} {activeIndex} onSelect={selectEntry} onStage={handleStage} onUnstage={handleUnstage} />
				{/if}
				<div class="pane-wrap">
					<div class="pane" bind:this={paneElement} onscroll={handlePaneScroll}>
						{#each entries as entry, index (entry.id)}
							<div class="file-section" bind:this={fileSectionElements[index]}>
								<DiffFile
									{entry}
									{stageable}
									{layout}
									collapsed={isCollapsed(entry)}
									onToggleCollapsed={() => setCollapsed(entry.id, !isCollapsed(entry))}
									onStage={handleStage}
									onUnstage={handleUnstage}
									searchRanges={searchRangesFor(entry.id)}
									expandedKeys={expandedKeysFor(entry.id)}
									onExpandedKeysChange={(keys) => {
										expandedKeysByEntry = new Map(expandedKeysByEntry).set(entry.id, keys);
									}}
								/>
							</div>
						{/each}
					</div>
					<OverviewRuler marks={rulerMarks} scrollElement={paneElement} onMarkActivate={handleMarkActivate} />
				</div>
			</div>
		{/if}

		<footer class="keys">
			<span><kbd>n</kbd><kbd>p</kbd> change</span>
			<span><kbd>]</kbd><kbd>[</kbd> file</span>
			{#if stageable}
				<span><kbd>s</kbd> stage / unstage</span>
				<span><kbd>R</kbd> refresh</span>
			{/if}
			<span><kbd>Ctrl+F</kbd> search</span>
			<span><kbd>Esc</kbd> close</span>
		</footer>
	{/if}
</div>

<style>
	.diff-view {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
		height: 100%;
		width: 100%;
		background: var(--color-bg);
		color: var(--color-fg);
		font-family: var(--font-sans, system-ui);
	}

	.loading,
	.error,
	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		flex: 1;
		gap: 16px;
		padding: 32px;
		text-align: center;
	}

	.spinner {
		width: 32px;
		height: 32px;
		border: 3px solid var(--color-border);
		border-top-color: var(--color-accent);
		border-radius: 50%;
		animation: spin 1s linear infinite;
	}

	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}

	.error-title {
		font-size: 16px;
		font-weight: 600;
		color: var(--color-error, #dc2626);
	}

	.error-message {
		font-size: 14px;
		color: var(--color-fg-alt);
		font-family: var(--font-mono);
		white-space: pre-wrap;
		max-width: 90%;
	}

	.diff-header {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 0 16px;
		height: 44px;
		flex-shrink: 0;
		border-bottom: 1px solid var(--color-border);
		background: var(--color-bg);
	}

	.badge {
		font-size: 11px;
		font-family: var(--font-mono);
		font-weight: 700;
		color: var(--color-accent);
		background: color-mix(in oklab, var(--color-accent) 12%, transparent);
		padding: 2px 5px;
		border-radius: 3px;
		line-height: 1;
	}

	.title {
		font-family: var(--font-mono);
		font-size: 13px;
		font-weight: 500;
		white-space: nowrap;
	}

	.dir {
		font-size: 12px;
		color: var(--color-fg-alt);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		flex: 1;
		min-width: 0;
	}

	.progress {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 12px;
		color: var(--color-muted);
		white-space: nowrap;
	}

	.progress .bar {
		width: 48px;
		height: 4px;
		border-radius: 2px;
		background: var(--color-border);
		overflow: hidden;
	}

	.progress .bar span {
		display: block;
		height: 100%;
		background: var(--color-accent);
	}

	.stats {
		font-family: var(--font-mono);
		font-size: 12px;
		white-space: nowrap;
	}

	.additions {
		color: var(--color-accent);
	}

	.deletions {
		color: var(--color-error, #e5484d);
	}

	.toggle {
		display: flex;
		border: 1px solid var(--color-border);
		border-radius: 6px;
		overflow: hidden;
		flex-shrink: 0;
	}

	.toggle button {
		font-size: 11px;
		padding: 4px 8px;
		border: none;
		background: var(--color-bg);
		color: var(--color-fg-alt);
		cursor: pointer;
	}

	.toggle button.on {
		background: var(--color-accent);
		color: var(--color-bg);
	}

	.icon-button {
		background: transparent;
		border: none;
		color: var(--color-fg);
		cursor: pointer;
		padding: 6px 8px;
		font-size: 16px;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 4px;
		flex-shrink: 0;
	}

	.icon-button:hover {
		background: var(--color-surface);
	}

	.banner {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 6px 16px;
		font-size: 12px;
		background: color-mix(in oklab, var(--color-accent) 14%, var(--color-bg));
		border-bottom: 1px solid var(--color-border);
		color: var(--color-fg);
		flex-shrink: 0;
	}

	.banner.error {
		background: color-mix(in oklab, var(--color-error, #e5484d) 22%, var(--color-bg));
	}

	.banner button {
		margin-left: auto;
		font-size: 11px;
		padding: 2px 8px;
		border-radius: 4px;
		border: 1px solid var(--color-border);
		background: var(--color-bg);
		color: var(--color-fg);
		cursor: pointer;
	}

	.body {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: row;
	}

	.pane-wrap {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: row;
	}

	.pane {
		flex: 1;
		min-width: 0;
		overflow: auto;
	}

	.file-section {
		border-bottom: 1px solid var(--color-border);
	}

	.keys {
		height: 24px;
		flex-shrink: 0;
		display: flex;
		align-items: center;
		gap: 16px;
		padding: 0 16px;
		font-size: 11px;
		color: var(--color-muted);
		border-top: 1px solid var(--color-border);
		background: var(--color-bg-alt);
	}

	.keys kbd {
		font-family: var(--font-mono);
		font-size: 10px;
		padding: 1px 4px;
		border-radius: 3px;
		border: 1px solid var(--color-border);
		margin-right: 2px;
	}
</style>
