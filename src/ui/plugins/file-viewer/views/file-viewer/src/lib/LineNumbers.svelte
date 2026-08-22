<script lang="ts">
	interface Props {
		lineCount: number;
	}

	let { lineCount }: Props = $props();

	let lineNumbers = $derived(Array.from({ length: lineCount }, (_, i) => i + 1));
</script>

<div class="line-numbers">
	{#each lineNumbers as lineNumber}
		<div class="line-number">{lineNumber}</div>
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
