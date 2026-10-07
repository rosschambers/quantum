<script lang="ts">
	// Test-only harness: renders VirtualScroller with a real Svelte-compiled
	// snippet (as every production consumer does) so scrollToIndex can be
	// exercised through testing-library without constructing a snippet by
	// hand — `createRawSnippet`'s SSR-shaped output does not match what a
	// client-compiled `{@render children(...)}` call site expects.
	import VirtualScroller from './VirtualScroller.svelte';

	interface Props {
		lines: string[];
		lineHeight: number;
		bufferLines?: number;
		scrollToIndex?: number;
		scrollRequest?: unknown;
		verticalPadding?: number;
	}

	let { lines, lineHeight, bufferLines, scrollToIndex, scrollRequest, verticalPadding }: Props = $props();
</script>

<VirtualScroller {lines} {lineHeight} {bufferLines} {scrollToIndex} {scrollRequest} {verticalPadding}>
	{#snippet children(props)}
		<div class="harness-content">
			{#each props.visibleLines as line}
				<div class="harness-line">{line}</div>
			{/each}
		</div>
	{/snippet}
</VirtualScroller>
