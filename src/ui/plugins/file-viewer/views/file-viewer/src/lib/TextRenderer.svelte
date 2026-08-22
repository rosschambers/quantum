<script lang="ts">
	import VirtualScroller from './VirtualScroller.svelte';

	interface Props {
		content: string;
	}

	const { content }: Props = $props();

	let lines = $derived(content.split('\n'));
	let lineCount = $derived(lines.length);
	let useVirtualScrolling = $derived(lineCount > 500);
</script>

{#if useVirtualScrolling}
	<VirtualScroller {lines} lineHeight={20.8} bufferLines={50}>
		{#snippet children(props)}
			<div class="text-content">
				<pre>{#each props.visibleLines as line}{line}
{/each}</pre>
			</div>
		{/snippet}
	</VirtualScroller>
{:else}
	<pre class="text-content">{content}</pre>
{/if}

<style>
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
</style>
