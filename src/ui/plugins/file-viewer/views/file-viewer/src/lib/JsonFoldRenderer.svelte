<script lang="ts">
	import { highlightCode } from './highlighter';

	interface Props {
		content: string;
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

	let { content }: Props = $props();

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
</script>

<div class="json-fold-renderer">
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
	<div class="code-content">
		<pre><code class="hljs">{#each visibleLines as line, index}{@html highlightCode(line.collapsed ? line.content + (line.collapsedSummary ?? '') : line.content, 'json')}{#if index < visibleLines.length - 1}{'\n'}{/if}{/each}</code></pre>
	</div>
</div>

<style>
	@import './highlight-theme.css';

	.json-fold-renderer {
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
