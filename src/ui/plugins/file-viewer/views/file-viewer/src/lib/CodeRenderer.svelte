<script lang="ts">
	import { highlightCode, highlightLineWithMatches } from './highlighter';
	import { buildCodeFoldModel, foldAncestors } from './fold-model';
	import { findMatchesInLines, type MatchRange } from './search';
	import LineNumbers from './LineNumbers.svelte';
	import VirtualScroller from './VirtualScroller.svelte';

	interface Props {
		content: string;
		language?: string;
		/** Literal, case-insensitive search query. Empty string means inactive. */
		query?: string;
		/** Index into this component's own match list that is "current". */
		currentMatchIndex?: number | null;
		/** Fired whenever the computed match count changes. */
		onMatchCount?: (count: number) => void;
		navigationRevision?: number;
		/** The scrolling container, exposed for the shared overview ruler. */
		scrollElement?: HTMLElement | null;
	}

	interface CodeVisibleLine {
		lineNumber: number;
		html: string;
		foldable: boolean;
		collapsed: boolean;
	}

	let { content, language, query = '', currentMatchIndex = null, onMatchCount, navigationRevision = 0, scrollElement = $bindable(null) }: Props = $props();

	let lines = $derived(content.replace(/\n+$/, '').split('\n'));
	let lineCount = $derived(lines.length);
	let useVirtualScrolling = $derived(lineCount > 500);

	// Code folding for non-virtual-scrolling path
	let codeFoldModel = $derived(buildCodeFoldModel(lines));
	let codeFoldState: Map<number, boolean> = $state(new Map());

	$effect(() => {
		content;
		language;
		codeFoldState = new Map();
	});

	let matches = $derived(query ? findMatchesInLines(lines, query) : []);
	let matchesByLine = $derived.by(() => {
		const map = new Map<number, MatchRange[]>();
		for (const match of matches) {
			const existing = map.get(match.lineIndex);
			if (existing) {
				existing.push(match.range);
			} else {
				map.set(match.lineIndex, [match.range]);
			}
		}
		return map;
	});
	let currentMatch = $derived(
		currentMatchIndex !== null && currentMatchIndex >= 0 && currentMatchIndex < matches.length
			? matches[currentMatchIndex]
			: null,
	);

	$effect(() => {
		onMatchCount?.(matches.length);
	});

	function matchesForLine(lineIndex: number): MatchRange[] {
		return matchesByLine.get(lineIndex) ?? [];
	}

	function currentRangeForLine(lineIndex: number): MatchRange | null {
		return currentMatch && currentMatch.lineIndex === lineIndex ? currentMatch.range : null;
	}

	// Depends only on lines/language, never on the search query or current
	// match, so a keystroke never re-highlights the whole file.
	let baseHighlightedLines = $derived(lines.map((line) => highlightCode(line, language)));

	function highlightedLineHtml(lineIndex: number, text: string): string {
		const lineMatches = matchesForLine(lineIndex);
		if (lineMatches.length === 0) {
			return baseHighlightedLines[lineIndex];
		}
		return highlightLineWithMatches(text, language, lineMatches, currentRangeForLine(lineIndex));
	}

	let codeContentElement: HTMLDivElement | undefined = $state(undefined);
	let scrollRequest = $derived({ match: currentMatch, revision: navigationRevision });

	// Reveal the current match: scroll it into view, and for the non-virtual
	// (foldable) path, expand EVERY fold enclosing its line first — not just
	// the innermost one. CodeRenderer's own collapsed-fold line-walk jumps
	// straight from a collapsed fold's header to its endLine + 1, so a line
	// nested inside several folds stays out of codeVisibleLines entirely
	// unless every enclosing fold is expanded, not only the tightest.
	$effect(() => {
		void navigationRevision;
		if (!currentMatch) return;
		const line = currentMatch.lineIndex;

		if (useVirtualScrolling) {
			return;
		}

		const ancestors = foldAncestors(codeFoldModel, line);
		if (ancestors.length > 0) {
			let changed = false;
			const next = new Map(codeFoldState);
			for (const fold of ancestors) {
				if (next.get(fold.startLine) !== false) {
					next.set(fold.startLine, false);
					changed = true;
				}
			}
			if (changed) {
				codeFoldState = next;
			}
		}

		// The fold-state write above only takes effect in the DOM on the next
		// microtask (codeVisibleLines is a $derived recomputed through
		// Svelte's own render cycle, not synchronously within this effect
		// body) — defer the scroll-into-view query until then.
		const targetLineNumber = line + 1;
		queueMicrotask(() => {
			codeContentElement?.querySelector(`[data-line="${targetLineNumber}"]`)?.scrollIntoView({ block: 'center' });
		});
	});

	let codeVisibleLines: CodeVisibleLine[] = $derived.by(() => {
		const result: CodeVisibleLine[] = [];
		let i = 0;
		while (i < lines.length) {
			const fold = codeFoldModel.get(i);
			const isCollapsed = fold && codeFoldState.get(i) === true;

			if (isCollapsed && fold) {
				const headerMatches = matchesForLine(i);
				result.push({
					lineNumber: i + 1,
					html:
						headerMatches.length === 0
							? highlightCode(lines[i] + ' ...', language)
							: `${highlightLineWithMatches(lines[i], language, headerMatches, currentRangeForLine(i))} ...`,
					foldable: true,
					collapsed: true,
				});
				i = fold.endLine + 1;
			} else {
				result.push({
					lineNumber: i + 1,
					html: highlightedLineHtml(i, lines[i]),
					foldable: !!fold,
					collapsed: false,
				});
				i++;
			}
		}
		return result;
	});

	function toggleCodeFold(lineIndex: number) {
		const current = codeFoldState.get(lineIndex) ?? false;
		codeFoldState.set(lineIndex, !current);
		codeFoldState = new Map(codeFoldState);
	}
</script>

<div class="code-renderer">
	{#if useVirtualScrolling}
		<VirtualScroller
			{lines}
			lineHeight={21}
			verticalPadding={12}
			bufferLines={50}
			scrollToIndex={currentMatch?.lineIndex}
			{scrollRequest}
		>
			{#snippet children(props)}
				<div class="virtual-code-lines">
					<LineNumbers lineCount={props.visibleEnd - props.visibleStart} startLine={props.visibleStart + 1} lineHeight={props.lineHeight} verticalPadding={0} />
					<div class="code-content" style="padding-top: 0; padding-bottom: 0;">
						<pre><code class="hljs">{#each props.visibleLines as line, index}<span class="code-line" style={props.rowStyle} data-line={props.visibleStart + index + 1}>{@html highlightedLineHtml(props.visibleStart + index, line)}{#if props.visibleStart + index < lines.length - 1}{'\n'}{/if}</span>{/each}</code></pre>
					</div>
				</div>
			{/snippet}
		</VirtualScroller>
	{:else}
		<div class="code-scroller" bind:this={scrollElement}>
			<div class="gutter">
				{#each codeVisibleLines as line}
					<div class="gutter-line">
						{#if line.foldable}
							<button
								class="fold-marker"
								class:collapsed={line.collapsed}
								onclick={() => toggleCodeFold(line.lineNumber - 1)}
								title={line.collapsed ? 'Expand' : 'Collapse'}
							>
								<svg width="8" height="8" viewBox="0 0 8 8">
									{#if line.collapsed}
										<polygon points="0,0 8,4 0,8" fill="currentColor" />
									{:else}
										<polygon points="0,0 8,0 4,8" fill="currentColor" />
									{/if}
								</svg>
							</button>
						{/if}
						<span class="line-number">{line.lineNumber}</span>
					</div>
				{/each}
			</div>
			<div class="code-content" bind:this={codeContentElement}>
				<pre><code class="hljs">{#each codeVisibleLines as line, index}<span class="code-line" data-line={line.lineNumber}>{@html line.html}</span>{#if index < codeVisibleLines.length - 1}{'\n'}{/if}{/each}</code></pre>
			</div>
		</div>
	{/if}
</div>

<style>
	@import './highlight-theme.css';

	.code-renderer {
		display: flex;
		height: 100%;
		font-family: var(--font-mono);
		font-size: 13px;
		line-height: 1.6;
		background: var(--color-bg);
	}

	.virtual-code-lines {
		display: flex;
	}

	.code-scroller {
		display: flex;
		width: max-content;
		min-width: 100%;
		height: 100%;
		overflow: auto;
	}

	.gutter {
		position: sticky;
		left: 0;
		z-index: 1;
		background: var(--color-bg);
		border-right: 1px solid var(--color-border);
		padding: 12px 12px 12px 8px;
		text-align: right;
		flex-shrink: 0;
		user-select: none;
	}

	.gutter-line {
		height: 20.8px;
		line-height: 1.6;
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 4px;
	}

	.line-number {
		color: color-mix(in srgb, var(--color-muted) 50%, transparent);
	}

	.fold-marker {
		background: none;
		border: none;
		cursor: pointer;
		color: color-mix(in srgb, var(--color-muted) 60%, transparent);
		padding: 0;
		display: flex;
		align-items: center;
		font-size: 8px;
	}

	.fold-marker:hover {
		color: var(--color-muted);
	}

	.code-content {
		flex: 1;
		overflow: visible;
		padding: 12px 24px;
	}

	pre {
		margin: 0;
		padding: 0;
		font-family: inherit;
		font-size: inherit;
		line-height: inherit;
		color: inherit;
		background: transparent;
		border: none;
		white-space: pre;
		word-wrap: normal;
		word-break: normal;
		overflow-wrap: normal;
	}

	code {
		font-family: inherit;
		font-size: inherit;
		line-height: inherit;
		color: inherit;
	}

	:global(.code-content mark.search-match) {
		background: color-mix(in oklab, var(--color-accent) 35%, transparent);
		color: inherit;
		border-radius: 2px;
	}

	:global(.code-content mark.search-match-current) {
		background: var(--color-accent);
		color: var(--color-bg);
	}
</style>
