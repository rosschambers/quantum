<script lang="ts">
	import { highlightCode } from './highlighter';
	import LineNumbers from './LineNumbers.svelte';
	import VirtualScroller from './VirtualScroller.svelte';

	interface Props {
		content: string;
		language?: string;
	}

	let { content, language }: Props = $props();

	let lines = $derived(content.replace(/\n+$/, '').split('\n'));
	let lineCount = $derived(lines.length);
	let useVirtualScrolling = $derived(lineCount > 500);

	let highlightedLines = $derived.by(() => {
		return lines.map((line) => highlightCode(line, language));
	});

	let visibleStart = $state(0);
	let visibleEnd = $state(0);

	$effect(() => {
		visibleEnd = lineCount;
	});
</script>

<div class="code-renderer">
	{#if useVirtualScrolling}
		<LineNumbers lineCount={visibleEnd - visibleStart} />
		<VirtualScroller {lines} lineHeight={20.8} bufferLines={50}>
			{#snippet children(props)}
				{@const _ = (visibleStart = props.visibleStart, visibleEnd = props.visibleEnd)}
				<div class="code-content">
					<pre><code class="hljs">{#each props.visibleLines as line, index}{@html line}{#if index < props.visibleLines.length - 1}{'\n'}{/if}{/each}</code></pre>
				</div>
			{/snippet}
		</VirtualScroller>
	{:else}
		<LineNumbers {lineCount} />
		<div class="code-content">
			<pre><code class="hljs">{@html highlightedLines.join('\n')}</code></pre>
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
		overflow: hidden;
	}

	.code-content {
		flex: 1;
		overflow: auto;
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
</style>
