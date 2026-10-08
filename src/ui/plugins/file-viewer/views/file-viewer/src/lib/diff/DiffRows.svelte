<script lang="ts">
	// Renders a diffed file's rows, unified or split, from the pure pieces
	// built elsewhere: `rows.ts` (DiffRow / toSplitRows), `highlightLines.ts`
	// (per-line syntax tokens + renderTokens for emphasis/search layering),
	// `copyText.ts` (clean clipboard text from the DOM). This component owns
	// no diff logic itself — only markup, copy hygiene, and split-view
	// selection locking / scroll sync.
	//
	// `searchRanges` shape: per-side maps from a line's old/new index (the
	// same index space as `oldTokens`/`newTokens`) to that line's search
	// ranges and, if one of them is the current match, which range that is.
	// `DiffFile.svelte` and `DiffView.svelte` import the `SearchRanges` /
	// `LineSearchRanges` types below directly from this component — they are
	// not redeclared anywhere.
	import type { DiffRow, SplitViewRow } from './rows';
	import { toSplitRows } from './rows';
	import { renderTokens, type DiffToken, type EmphasisLayer } from './highlightLines';
	import { collectSelectedCells, buildClipboardText } from './copyText';

	export interface LineSearchRanges {
		ranges: readonly [number, number][];
		currentRange: readonly [number, number] | null;
	}
	export interface SearchRanges {
		old: Map<number, LineSearchRanges>;
		new: Map<number, LineSearchRanges>;
	}

	interface Props {
		rows: DiffRow[];
		layout: 'unified' | 'split';
		oldTokens: DiffToken[][];
		newTokens: DiffToken[][];
		searchRanges?: SearchRanges;
		onExpand: (key: string) => void;
		onRecollapse: (key: string) => void;
	}

	let { rows, layout, oldTokens, newTokens, searchRanges, onExpand, onRecollapse }: Props = $props();

	type Side = 'old' | 'new';
	type RowKind = 'context' | 'removed' | 'added';

	let rootElement: HTMLDivElement | undefined = $state(undefined);
	let lockedSide: Side | null = $state(null);

	let digitWidth = $derived(String(Math.max(oldTokens.length, newTokens.length, 1)).length);

	function searchEntryFor(side: Side, index: number): LineSearchRanges | undefined {
		return side === 'old' ? searchRanges?.old.get(index) : searchRanges?.new.get(index);
	}

	function emphasisClassFor(kind: RowKind): string {
		return kind === 'removed' ? 'ix-del' : 'ix-add';
	}

	function buildLayers(
		emphasis: readonly (readonly [number, number])[] | null | undefined,
		kind: RowKind,
		side: Side,
		index: number,
	): EmphasisLayer[] {
		const layers: EmphasisLayer[] = [];
		if (emphasis && emphasis.length > 0) {
			layers.push({ ranges: emphasis as [number, number][], className: emphasisClassFor(kind) });
		}
		const searchEntry = searchEntryFor(side, index);
		if (searchEntry && searchEntry.ranges.length > 0) {
			layers.push({ ranges: searchEntry.ranges as [number, number][], className: 'search-match' });
		}
		if (searchEntry?.currentRange) {
			layers.push({ ranges: [searchEntry.currentRange as [number, number]], className: 'search-match-current' });
		}
		return layers;
	}

	function cellHtml(
		side: Side,
		index: number,
		kind: RowKind,
		emphasis: readonly (readonly [number, number])[] | null | undefined,
	): string {
		const tokens = (side === 'old' ? oldTokens[index] : newTokens[index]) ?? [];
		return renderTokens(tokens, buildLayers(emphasis, kind, side, index));
	}

	function lineText(side: Side, index: number): string {
		const tokens = (side === 'old' ? oldTokens[index] : newTokens[index]) ?? [];
		return tokens.map((token) => token.text).join('');
	}

	function hiddenLinesJson(firstNewIndex: number, hiddenCount: number): string {
		const lines: string[] = [];
		for (let offset = 0; offset < hiddenCount; offset++) {
			lines.push(lineText('new', firstNewIndex + offset));
		}
		return JSON.stringify(lines);
	}

	function unchangedLabel(count: number): string {
		return `\u2195 ${count} unchanged line${count === 1 ? '' : 's'}`;
	}

	function expandedLabel(count: number): string {
		return `Hide ${count} expanded line${count === 1 ? '' : 's'}`;
	}

	function recollapseArrow(position: 'top' | 'bottom'): string {
		return position === 'top' ? '\u25B4' : '\u25BE';
	}

	function lineNumberText(value: number | ''): string {
		return value === '' ? '' : String(value);
	}

	// Unified row number text: context/added show the new index, removed
	// shows blank on the new column; context/removed show the old index,
	// added shows blank on the old column.
	function oldNumberFor(row: DiffRow): number | '' {
		if (row.kind === 'added') {
			return '';
		}
		return row.kind === 'context' || row.kind === 'removed' ? row.oldIndex + 1 : '';
	}
	function newNumberFor(row: DiffRow): number | '' {
		if (row.kind === 'removed') {
			return '';
		}
		return row.kind === 'context' || row.kind === 'added' ? row.newIndex + 1 : '';
	}

	// ONE pair of columns for the whole file (not one per hunk): every row,
	// in order, lives in BOTH `.column.old` and `.column.new` — context and
	// change halves render their real content on whichever side has it (an
	// inert `.half.empty` filler otherwise), and a collapsed/recollapse row
	// renders its real, clickable label in the old column with a same-height
	// inert spacer in the new column (never a second `.collapsed`, so
	// `data-hidden-lines` exists exactly once for `copyText.ts` to read).
	// Fixed row heights (`.row`, `.half`: `min-height: 20.8px`) keep the two
	// columns aligned without wrapping. A single scroll-mirroring handler per
	// column keeps the whole file in horizontal sync, not just one hunk.
	let splitRows = $derived.by(() => toSplitRows(rows));

	let oldColumnElement: HTMLDivElement | undefined = $state(undefined);
	let newColumnElement: HTMLDivElement | undefined = $state(undefined);

	function mirrorScroll(source: Side): void {
		const from = source === 'old' ? oldColumnElement : newColumnElement;
		const to = source === 'old' ? newColumnElement : oldColumnElement;
		if (from && to) {
			to.scrollLeft = from.scrollLeft;
		}
	}

	function handlePointerDown(side: Side): void {
		lockedSide = side;
	}

	function handleExpandKey(event: KeyboardEvent, key: string): void {
		if (event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			onExpand(key);
		}
	}

	function handleRecollapseKey(event: KeyboardEvent, key: string): void {
		if (event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			onRecollapse(key);
		}
	}

	function handleCopy(event: ClipboardEvent): void {
		const selection = window.getSelection();
		if (!selection || !rootElement) {
			return;
		}
		const cells = collectSelectedCells(selection, rootElement);
		const text = buildClipboardText(cells, { layout, lockedSide });
		event.clipboardData?.setData('text/plain', text);
		event.preventDefault();
	}
</script>

<div
	class="diff-root {layout}"
	class:pick-old={lockedSide === 'old'}
	class:pick-new={lockedSide === 'new'}
	bind:this={rootElement}
	oncopy={handleCopy}
>
	{#if layout === 'unified'}
		{#each rows as row, index (index)}
			{#if row.kind === 'collapsed'}
				<div
					class="collapsed"
					role="button"
					tabindex="0"
					data-hidden-lines={hiddenLinesJson(row.firstNewIndex, row.hiddenCount)}
					onclick={() => onExpand(row.key)}
					onkeydown={(event) => handleExpandKey(event, row.key)}
				>
					<span class="count">{unchangedLabel(row.hiddenCount)}</span>
					{#if row.scopeLine}<span class="scope">{@html row.scopeLine}</span>{/if}
				</div>
			{:else if row.kind === 'recollapse'}
				<div class="recollapse" role="button" tabindex="0" onclick={() => onRecollapse(row.key)} onkeydown={(event) => handleRecollapseKey(event, row.key)}>
					{recollapseArrow(row.position)} {expandedLabel(row.count)}
				</div>
			{:else}
				<div class="row {row.kind}">
					<span class="line-number" style={`width: calc(${digitWidth}ch + 12px)`}>{lineNumberText(oldNumberFor(row))}</span>
					<span class="line-number last" style={`width: calc(${digitWidth}ch + 12px)`}>{lineNumberText(newNumberFor(row))}</span>
					<span
						class="code"
						data-side={row.kind === 'removed' ? 'old' : 'new'}
						data-row-kind={row.kind}
						>{@html cellHtml(
							row.kind === 'removed' ? 'old' : 'new',
							row.kind === 'removed' ? row.oldIndex : row.kind === 'added' ? row.newIndex : row.newIndex,
							row.kind,
							row.kind === 'context' ? null : row.emphasis,
						)}</span
					>
				</div>
			{/if}
		{/each}
	{:else}
		<div class="split-view">
			<div
				class="column old"
				role="presentation"
				bind:this={oldColumnElement}
				onpointerdown={() => handlePointerDown('old')}
				onscroll={() => mirrorScroll('old')}
			>
				{#each splitRows as splitRow, rowIndex (rowIndex)}
					{#if splitRow.full && splitRow.left?.kind === 'collapsed'}
						{@const collapsedRow = splitRow.left}
						<div
							class="collapsed"
							role="button"
							tabindex="0"
							data-hidden-lines={hiddenLinesJson(collapsedRow.firstNewIndex, collapsedRow.hiddenCount)}
							onclick={() => onExpand(collapsedRow.key)}
							onkeydown={(event) => handleExpandKey(event, collapsedRow.key)}
						>
							<span class="count">{unchangedLabel(collapsedRow.hiddenCount)}</span>
							{#if collapsedRow.scopeLine}<span class="scope">{@html collapsedRow.scopeLine}</span>{/if}
						</div>
					{:else if splitRow.full && splitRow.left?.kind === 'recollapse'}
						{@const recollapseRow = splitRow.left}
						<div
							class="recollapse"
							role="button"
							tabindex="0"
							onclick={() => onRecollapse(recollapseRow.key)}
							onkeydown={(event) => handleRecollapseKey(event, recollapseRow.key)}
						>
							{recollapseArrow(recollapseRow.position)} {expandedLabel(recollapseRow.count)}
						</div>
					{:else if splitRow.left}
						{@const leftRow = splitRow.left}
						<div class="half {leftRow.kind}">
							<span class="line-number last" style={`width: calc(${digitWidth}ch + 12px)`}>{leftRow.kind === 'added' ? '' : leftRow.oldIndex + 1}</span>
							<span
								class="code"
								data-side="old"
								data-row-kind={leftRow.kind}
								>{@html cellHtml('old', leftRow.oldIndex, leftRow.kind as RowKind, leftRow.kind === 'removed' ? leftRow.emphasis : null)}</span
							>
						</div>
					{:else}
						<div class="half empty"></div>
					{/if}
				{/each}
			</div>
			<div
				class="column new"
				role="presentation"
				bind:this={newColumnElement}
				onpointerdown={() => handlePointerDown('new')}
				onscroll={() => mirrorScroll('new')}
			>
				{#each splitRows as splitRow, rowIndex (rowIndex)}
					{#if splitRow.full}
						{@const fullRow = splitRow.left}
						<!-- Inert spacer mirroring the old column's collapsed/recollapse
						     label at the same height, so rows stay aligned. Never class
						     `collapsed` — `copyText.ts` relies on exactly one real
						     `.collapsed` element (in the old column) for `data-hidden-lines`. -->
						<div
							class="spacer {fullRow?.kind === 'collapsed' ? 'spacer-collapsed' : 'spacer-recollapse'}"
							aria-hidden="true"
						></div>
					{:else if splitRow.right}
						{@const rightRow = splitRow.right}
						<div class="half {rightRow.kind}">
							<span class="line-number last" style={`width: calc(${digitWidth}ch + 12px)`}>{rightRow.kind === 'removed' ? '' : rightRow.newIndex + 1}</span>
							<span
								class="code"
								data-side="new"
								data-row-kind={rightRow.kind}
								>{@html cellHtml('new', rightRow.newIndex, rightRow.kind as RowKind, rightRow.kind === 'added' ? rightRow.emphasis : null)}</span
							>
						</div>
					{:else}
						<div class="half empty"></div>
					{/if}
				{/each}
			</div>
		</div>
	{/if}
</div>

<style>
	@import '../highlight-theme.css';

	.diff-root {
		user-select: none;
		-webkit-user-select: none;
		font-family: var(--font-mono);
		font-size: 13px;
		line-height: 1.6;
		tab-size: 4;
	}

	.diff-root.unified {
		width: max-content;
		min-width: 100%;
	}

	.diff-root :global(.code) {
		user-select: text;
		-webkit-user-select: text;
	}

	.diff-root.pick-old .column.new :global(.code) {
		user-select: none;
		-webkit-user-select: none;
	}

	.diff-root.pick-new .column.old :global(.code) {
		user-select: none;
		-webkit-user-select: none;
	}

	.row,
	.half {
		display: flex;
		min-height: 20.8px;
	}

	.line-number {
		flex-shrink: 0;
		text-align: right;
		padding: 0 8px 0 4px;
		color: color-mix(in srgb, var(--color-muted) 50%, transparent);
		user-select: none;
	}

	.line-number.last {
		border-right: 1px solid var(--color-border);
	}

	.code {
		padding: 0 16px 0 10px;
		white-space: pre;
		flex: 1;
		color: var(--color-fg-alt);
	}

	.row.removed,
	.half.removed {
		background: color-mix(in oklab, var(--color-error, #e5484d) 14%, transparent);
		box-shadow: inset 3px 0 0 var(--color-error, #e5484d);
	}

	.row.added,
	.half.added {
		background: color-mix(in oklab, var(--color-accent) 14%, transparent);
		box-shadow: inset 3px 0 0 var(--color-accent);
	}

	.diff-root :global(.ix-add) {
		background: color-mix(in oklab, var(--color-accent) 35%, transparent);
		border-radius: 2px;
	}

	.diff-root :global(.ix-del) {
		background: color-mix(in oklab, var(--color-error, #e5484d) 35%, transparent);
		border-radius: 2px;
	}

	.diff-root :global(.search-match) {
		background: color-mix(in oklab, var(--color-accent) 35%, transparent);
		border-radius: 2px;
	}

	.diff-root :global(.search-match-current) {
		background: var(--color-accent);
		color: var(--color-bg);
	}

	.collapsed {
		display: flex;
		align-items: center;
		gap: 10px;
		min-height: 26px;
		margin: 4px 0;
		padding: 0 12px;
		background: color-mix(in oklab, var(--color-surface) 70%, transparent);
		color: var(--color-muted);
		cursor: pointer;
		white-space: nowrap;
		overflow: hidden;
	}

	.collapsed:hover {
		color: var(--color-fg);
		background: var(--color-surface);
	}

	.collapsed .scope {
		color: var(--color-fg-alt);
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.recollapse {
		display: flex;
		align-items: center;
		gap: 6px;
		min-height: 18px;
		margin: 0;
		padding: 0 12px;
		font-size: 11px;
		background: color-mix(in oklab, var(--color-surface) 40%, transparent);
		color: var(--color-muted);
		cursor: pointer;
	}

	.split-view {
		display: flex;
	}

	.column {
		width: 50%;
		min-width: 0;
		overflow-x: auto;
		overflow-y: visible;
	}

	.column.new {
		border-left: 1px solid var(--color-border);
	}

	.half.empty {
		background: repeating-linear-gradient(
			135deg,
			transparent 0 6px,
			color-mix(in oklab, var(--color-border) 35%, transparent) 6px 7px
		);
	}

	/* Inert new-column counterparts of `.collapsed` / `.recollapse`, matching
	   their heights exactly so the two columns' rows stay aligned. Never
	   `.collapsed` (copyText.ts's selector) and never clickable. */
	.spacer-collapsed {
		min-height: 26px;
		margin: 4px 0;
	}

	.spacer-recollapse {
		min-height: 18px;
		margin: 0;
	}
</style>
