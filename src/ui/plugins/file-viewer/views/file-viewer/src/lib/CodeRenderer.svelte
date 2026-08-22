<script lang="ts">
	import { highlightCode } from './highlighter';
	import { buildCodeFoldModel, type CodeFoldRange } from './fold-model';
	import LineNumbers from './LineNumbers.svelte';
	import VirtualScroller from './VirtualScroller.svelte';

	interface Props {
		content: string;
		language?: string;
	}

	interface CodeVisibleLine {
		lineNumber: number;
		html: string;
		foldable: boolean;
		collapsed: boolean;
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

	// Code folding for non-virtual-scrolling path
	let codeFoldModel = $derived(buildCodeFoldModel(lines));
	let codeFoldState: Map<number, boolean> = $state(new Map());

	$effect(() => {
		content;
		language;
		codeFoldState = new Map();
	});

	let codeVisibleLines: CodeVisibleLine[] = $derived.by(() => {
		const result: CodeVisibleLine[] = [];
		let i = 0;
		while (i < lines.length) {
			const fold = codeFoldModel.get(i);
			const isCollapsed = fold && codeFoldState.get(i) === true;

			if (isCollapsed && fold) {
				result.push({
					lineNumber: i + 1,
					html: highlightCode(lines[i] + ' ...', language),
					foldable: true,
					collapsed: true,
				});
				i = fold.endLine + 1;
			} else {
				result.push({
					lineNumber: i + 1,
					html: highlightedLines[i],
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
		<div class="code-content">
			<pre><code class="hljs">{#each codeVisibleLines as line, index}{@html line.html}{#if index < codeVisibleLines.length - 1}{'\n'}{/if}{/each}</code></pre>
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

	.gutter {
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
