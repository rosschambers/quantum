<script lang="ts">
	import VirtualScroller from './VirtualScroller.svelte';
	import { highlightPlainLineWithMatches } from './highlighter';
	import { findMatchesInLines, type MatchRange } from './search';

	interface Props {
		content: string;
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

	let { content, query = '', currentMatchIndex = null, onMatchCount, navigationRevision = 0, scrollElement = $bindable(null) }: Props = $props();

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
	<VirtualScroller {lines} lineHeight={21} verticalPadding={32} bufferLines={50} scrollToIndex={currentMatch?.lineIndex} {scrollRequest}>
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
