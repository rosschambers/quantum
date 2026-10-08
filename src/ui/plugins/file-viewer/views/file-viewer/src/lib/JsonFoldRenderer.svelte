<script lang="ts">
	import { highlightCode, highlightLineWithMatches } from './highlighter';
	import { foldAncestors, type CodeFoldRange } from './fold-model';
	import { findMatchesInLines, type MatchRange } from './search';

	interface Props {
		content: string;
		/** Literal, case-insensitive search query. Empty string means inactive. */
		query?: string;
		/** Index into this component's own match list (against the PRETTY-PRINTED lines) that is "current". */
		currentMatchIndex?: number | null;
		/** Fired whenever the computed match count changes. */
		onMatchCount?: (count: number) => void;
		navigationRevision?: number;
	}

	interface FoldRange {
		startLine: number;
		endLine: number;
		count: number;
		closingChar: string;
	}

	interface VisibleLine {
		lineNumber: number;
		content: string;
		foldable: boolean;
		collapsed: boolean;
		collapsedSummary?: string;
	}

	let { content, query = '', currentMatchIndex = null, onMatchCount, navigationRevision = 0 }: Props = $props();

	let prettyContent = $derived.by(() => {
		try {
			return JSON.stringify(JSON.parse(content), null, 2);
		} catch {
			return content;
		}
	});

	let lines = $derived(prettyContent.split('\n'));

	function buildFoldModel(lines: string[]): Map<number, FoldRange> {
		const folds = new Map<number, FoldRange>();
		const stack: { line: number; char: string }[] = [];

		for (let i = 0; i < lines.length; i++) {
			const trimmed = lines[i].trim();
			const lastChar = trimmed[trimmed.length - 1];

			// Check for opening: line ends with { or [
			const opener = lastChar === '{' || lastChar === '[' ? lastChar : null;
			if (opener) {
				stack.push({ line: i, char: opener });
				continue;
			}

			// Check for closing: line starts with (whitespace +) } or ]
			const closingMatch = trimmed.match(/^[}\]],?$/);
			if (closingMatch && stack.length > 0) {
				const closingChar = trimmed[0];
				const expectedOpener = closingChar === '}' ? '{' : '[';
				// Find matching opener
				for (let s = stack.length - 1; s >= 0; s--) {
					if (stack[s].char === expectedOpener) {
						const start = stack[s].line;
						stack.splice(s, 1);
						// Count children (keys for objects, items for arrays)
						let count = 0;
						try {
							const blockText = lines.slice(start, i + 1).join('\n');
							// Extract just the JSON value starting from the opening brace
							const openIndex = blockText.indexOf(expectedOpener);
							const jsonFragment = blockText.substring(openIndex);
							const parsed = JSON.parse(jsonFragment.replace(/,\s*$/, ''));
							count = Array.isArray(parsed) ? parsed.length : Object.keys(parsed).length;
						} catch {
							// Fall back to line count
						}
						folds.set(start, {
							startLine: start,
							endLine: i,
							count,
							closingChar
						});
						break;
					}
				}
			}
		}
		return folds;
	}

	let foldState: Map<number, boolean> = $state(new Map());

	$effect(() => {
		prettyContent; // subscribe to changes
		foldState = new Map();
	});

	let foldModel = $derived(buildFoldModel(lines));

	let visibleLines: VisibleLine[] = $derived.by(() => {
		const result: VisibleLine[] = [];
		let i = 0;
		while (i < lines.length) {
			const fold = foldModel.get(i);
			const isCollapsed = fold && foldState.get(i) === true;

			if (isCollapsed && fold) {
				const label = fold.closingChar === '}' ? 'keys' : 'items';
				result.push({
					lineNumber: i + 1,
					content: lines[i],
					foldable: true,
					collapsed: true,
					collapsedSummary: ` ... ${fold.count} ${label} ${fold.closingChar}`
				});
				i = fold.endLine + 1;
			} else {
				result.push({
					lineNumber: i + 1,
					content: lines[i],
					foldable: !!fold,
					collapsed: false
				});
				i++;
			}
		}
		return result;
	});

	function toggleFold(lineIndex: number) {
		const current = foldState.get(lineIndex) ?? false;
		foldState.set(lineIndex, !current);
		foldState = new Map(foldState); // trigger reactivity
	}

	// Search matches are computed against the PRETTY-PRINTED `lines`, the
	// same authoritative array the gutter's own line numbers already key on
	// — never against the raw `content` (which may be minified onto a
	// single line). This is what keeps raw-vs-formatted line mapping correct
	// by construction: there is only ever one source of line numbering in
	// this component, and search shares it.
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

	// Depends only on lines, never on the search query or current match, so
	// a keystroke never re-highlights the whole file. Collapsed-line summary
	// suffixes are appended separately in lineHtml, below, since they are not
	// part of the cached base text.
	let baseHighlightedLines = $derived(lines.map((line) => highlightCode(line, 'json')));

	function lineHtml(lineIndex: number, text: string, summary?: string): string {
		const lineMatches = matchesByLine.get(lineIndex) ?? [];
		if (lineMatches.length === 0) {
			const base = summary !== undefined ? highlightCode(text + summary, 'json') : baseHighlightedLines[lineIndex];
			return base;
		}
		const current = currentMatch && currentMatch.lineIndex === lineIndex ? currentMatch.range : null;
		const highlighted = highlightLineWithMatches(text, 'json', lineMatches, current);
		return summary !== undefined ? highlighted + summary : highlighted;
	}

	let jsonContentElement: HTMLDivElement | undefined = $state(undefined);

	// Reveal the current match: expand EVERY fold enclosing its line (not
	// just the innermost one — JSON's own visibleLines walk jumps straight
	// from a collapsed fold's header to its endLine + 1, so a line nested
	// inside several folds stays out of the render entirely unless every
	// enclosing fold is expanded), then scroll it into view. JSON has no
	// virtualization threshold (per the design: it always renders every
	// line), so this is always a direct DOM scrollIntoView, never
	// VirtualScroller.scrollToIndex.
	$effect(() => {
		void navigationRevision;
		if (!currentMatch) return;
		const line = currentMatch.lineIndex;

		// foldAncestors is typed against fold-model's CodeFoldRange
		// (startLine/endLine only); project this component's own
		// start/end/count/closingChar FoldRange down to that shape rather
		// than widening foldAncestors' signature for one caller.
		const plainFoldModel = new Map<number, CodeFoldRange>();
		for (const [key, value] of foldModel) {
			plainFoldModel.set(key, { startLine: value.startLine, endLine: value.endLine });
		}
		const ancestors = foldAncestors(plainFoldModel, line);
		if (ancestors.length > 0) {
			let changed = false;
			const next = new Map(foldState);
			for (const fold of ancestors) {
				if (next.get(fold.startLine) !== false) {
					next.set(fold.startLine, false);
					changed = true;
				}
			}
			if (changed) {
				foldState = next;
			}
		}

		const targetLineNumber = line + 1;
		queueMicrotask(() => {
			jsonContentElement?.querySelector(`[data-line="${targetLineNumber}"]`)?.scrollIntoView({ block: 'center' });
		});
	});
</script>

<div class="json-fold-renderer">
	<div class="json-lines">
		<div class="gutter">
			{#each visibleLines as line}
				<div class="gutter-line">
					<span class="fold-slot">
						{#if line.foldable}
							<button
								class="fold-marker"
								class:collapsed={line.collapsed}
								onclick={() => toggleFold(line.lineNumber - 1)}
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
					</span>
					<span class="line-number">{line.lineNumber}</span>
				</div>
			{/each}
		</div>
		<div class="code-content" bind:this={jsonContentElement}>
			<pre><code class="hljs">{#each visibleLines as line, index}<span class="json-line" data-line={line.lineNumber}>{@html lineHtml(line.lineNumber - 1, line.content, line.collapsed ? line.collapsedSummary : undefined)}</span>{#if index < visibleLines.length - 1}{'\n'}{/if}{/each}</code></pre>
		</div>
	</div>
</div>

<style>
	@import './highlight-theme.css';

	.json-fold-renderer {
		height: 100%;
		font-family: var(--font-mono);
		font-size: 13px;
		line-height: 1.6;
		background: var(--color-bg);
		overflow: auto;
	}

	.json-lines {
		display: flex;
		width: max-content;
		min-width: 100%;
		min-height: 100%;
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
		overflow: hidden;
		user-select: none;
	}

	.gutter-line {
		height: 20.8px;
		line-height: 1.6;
		display: flex;
		align-items: center;
		gap: 4px;
	}

	.line-number {
		color: color-mix(in srgb, var(--color-muted) 50%, transparent);
		margin-left: auto;
	}

	.fold-slot {
		width: 12px;
		display: flex;
		align-items: center;
		justify-content: center;
		flex-shrink: 0;
	}

	.fold-marker {
		background: none;
		border: none;
		cursor: pointer;
		color: color-mix(in srgb, var(--color-muted) 60%, transparent);
		padding: 0;
		display: flex;
		align-items: center;
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
