<script lang="ts">
	import VirtualScroller from './VirtualScroller.svelte';
	import { highlightPlainLineWithMatches } from './highlighter';
	import { findMatchesInLines, type MatchRange } from './search';
	import { rowCenterFraction } from './overviewRuler';
	import { measureFractions, observeLayout } from './measureOffsets';

	interface Props {
		content: string;
		/** Literal, case-insensitive search query. Empty string means inactive. */
		query?: string;
		/** Index into this component's own match list that is "current". */
		currentMatchIndex?: number | null;
		/** Fired whenever the computed match count changes. */
		onMatchCount?: (count: number) => void;
		/**
		 * Fired whenever the match positions change (never when only
		 * currentMatchIndex changes). Positions are fractions along the
		 * scroll track, in the same order as onMatchCount's count, for the
		 * shared overview ruler.
		 */
		onMatchPositions?: (positions: Float64Array) => void;
		navigationRevision?: number;
		/** The scrolling container, exposed for the shared overview ruler. */
		scrollElement?: HTMLElement | null;
	}

	let { content, query = '', currentMatchIndex = null, onMatchCount, onMatchPositions, navigationRevision = 0, scrollElement = $bindable(null) }: Props = $props();

	let lines = $derived(content.split('\n'));
	let lineCount = $derived(lines.length);
	let useVirtualScrolling = $derived(lineCount > 500);

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

	// Virtual rows use fixed-height arithmetic (every line is exactly one
	// row), computed synchronously with no DOM access. Short (wrapping) text
	// has no fixed row height per line, so its positions are read from live
	// layout instead — see the measurement effects below.
	let virtualMatchPositions = $derived.by(() => {
		if (!useVirtualScrolling) return null;
		const positions = new Float64Array(matches.length);
		for (let index = 0; index < matches.length; index++) {
			positions[index] = rowCenterFraction(matches[index].lineIndex, lineCount, { paddingTop: 32, rowHeight: 21, paddingBottom: 32 });
		}
		return positions;
	});

	$effect(() => {
		if (virtualMatchPositions) onMatchPositions?.(virtualMatchPositions);
	});

	function measureWrappedMatchPositions(): Float64Array {
		const root = textContentElement;
		if (!root) return new Float64Array(0);
		const anchors = matches.map((match) => root.querySelector(`[data-line="${match.lineIndex + 1}"]`) ?? root);
		return measureFractions(root, anchors);
	}

	// Short (wrapping) text has no fixed per-line row height, so match
	// positions come from one batched live-layout read pass instead of
	// arithmetic — scheduled a frame after each search pass (never
	// synchronously inside the pass itself, and never on a mere
	// currentMatchIndex change, since this depends only on `matches`).
	$effect(() => {
		if (useVirtualScrolling) return;
		void matches;
		const frameId = requestAnimationFrame(() => {
			onMatchPositions?.(measureWrappedMatchPositions());
		});
		return () => cancelAnimationFrame(frameId);
	});

	// A persistent resize watch on the content root, independent of search
	// passes, so a layout change (reflow, font load, container resize)
	// re-measures the SAME matches without waiting for another keystroke.
	$effect(() => {
		if (useVirtualScrolling || !textContentElement) return;
		return observeLayout(textContentElement, () => {
			onMatchPositions?.(measureWrappedMatchPositions());
		});
	});

	function renderedLine(lineIndex: number, text: string): string {
		const lineMatches = matchesByLine.get(lineIndex) ?? [];
		const current = currentMatch && currentMatch.lineIndex === lineIndex ? currentMatch.range : null;
		return highlightPlainLineWithMatches(text, lineMatches, current);
	}

	let textContentElement: HTMLElement | undefined = $state(undefined);
	let scrollRequest = $derived({ match: currentMatch, revision: navigationRevision });

	$effect(() => {
		void navigationRevision;
		if (!currentMatch) return;
		const line = currentMatch.lineIndex;
		if (useVirtualScrolling) {
			return;
		}
		const targetLineNumber = line + 1;
		queueMicrotask(() => {
			textContentElement?.querySelector(`[data-line="${targetLineNumber}"]`)?.scrollIntoView({ block: 'center' });
		});
	});

</script>

{#if useVirtualScrolling}
	<VirtualScroller {lines} lineHeight={21} verticalPadding={32} bufferLines={50} scrollToIndex={currentMatch?.lineIndex} {scrollRequest} bind:container={scrollElement}>
		{#snippet children(props)}
			<pre class="text-content" style="padding: 0 32px; white-space: pre;">{#each props.visibleLines as line, index}<span class="text-line" style={props.rowStyle} data-line={props.visibleStart + index + 1}>{@html renderedLine(props.visibleStart + index, line)}{#if props.visibleStart + index < lines.length - 1}{'\n'}{/if}</span>{/each}</pre>
		{/snippet}
	</VirtualScroller>
{:else}
	<div class="text-scroller" bind:this={scrollElement}>
		{#if query && matches.length > 0}
			<pre class="text-content" bind:this={textContentElement}>{#each lines as line, index}<span class="text-line" data-line={index + 1}>{@html renderedLine(index, line)}</span>{#if index < lines.length - 1}{'\n'}{/if}{/each}</pre>
		{:else}
			<pre class="text-content">{content}</pre>
		{/if}
	</div>
{/if}

<style>
  .text-scroller {
    height: 100%;
    overflow: auto;
  }

  .text-content {
    font-family: var(--font-mono);
    font-size: 13px;
    line-height: 1.6;
    padding: 32px;
    color: var(--color-fg-alt);
    white-space: pre-wrap;
    tab-size: 2;
    overflow-wrap: break-word;
    margin: 0;
  }

  :global(.text-content mark.search-match) {
    background: color-mix(in oklab, var(--color-accent) 35%, transparent);
    color: inherit;
    border-radius: 2px;
  }

  :global(.text-content mark.search-match-current) {
    background: var(--color-accent);
    color: var(--color-bg);
  }
</style>
