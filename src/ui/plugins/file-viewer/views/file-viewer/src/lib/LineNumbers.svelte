<script lang="ts">
	interface Props {
		lineCount: number;
		startLine?: number;
		lineHeight?: number;
		verticalPadding?: number;
	}

	let { lineCount, startLine = 1, lineHeight = 20.8, verticalPadding = 12 }: Props = $props();

	let lineNumbers = $derived(Array.from({ length: lineCount }, (_, index) => index + startLine));
</script>

<div class="line-numbers" style:padding-top="{verticalPadding}px" style:padding-bottom="{verticalPadding}px">
	{#each lineNumbers as lineNumber}
		<div class="line-number" style="height: {lineHeight}px; line-height: {lineHeight}px; box-sizing: border-box;">{lineNumber}</div>
	{/each}
</div>

<style>
	.line-numbers {
		font-family: var(--font-mono);
		font-size: 13px;
		color: color-mix(in srgb, var(--color-muted) 50%, transparent);
		user-select: none;
		border-right: 1px solid var(--color-border);
		padding: 12px 12px 12px 16px;
		text-align: right;
		overflow: hidden;
		white-space: nowrap;
		flex-shrink: 0;
	}

	.line-number {
		line-height: 1.6;
		height: 20.8px;
	}
</style>
