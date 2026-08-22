<script lang="ts">
	interface Props {
		content: string;
		fileType: string;
		onformat: (formattedContent: string) => void;
	}

	let { content, fileType, onformat }: Props = $props();

	let dismissed = $state(false);

	let isMinified = $derived(
		fileType === 'json' &&
		content.length > 100 &&
		(content.match(/\n/g) || []).length < 3
	);

	let visible = $derived(isMinified && !dismissed);

	function handleFormat() {
		try {
			const formatted = JSON.stringify(JSON.parse(content), null, 2);
			onformat(formatted);
		} catch {
			// If JSON is invalid, just dismiss
		}
		dismissed = true;
	}

	function handleDismiss() {
		dismissed = true;
	}
</script>

{#if visible}
	<div class="format-banner">
		<span class="banner-text">This file appears minified.</span>
		<button class="format-button" onclick={handleFormat}>Format</button>
		<button class="dismiss-button" onclick={handleDismiss} title="Dismiss">&#x2715;</button>
	</div>
{/if}

<style>
	.format-banner {
		height: 36px;
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 0 16px;
		background: var(--color-bg-alt);
		border-bottom: 1px solid var(--color-border);
		flex-shrink: 0;
		font-size: 13px;
	}

	.banner-text {
		flex: 1;
	}

	.format-button {
		background: var(--color-accent);
		color: var(--color-bg);
		border: none;
		border-radius: 4px;
		padding: 4px 12px;
		cursor: pointer;
		font-size: 13px;
		font-weight: 500;
	}

	.format-button:hover {
		opacity: 0.9;
	}

	.dismiss-button {
		background: transparent;
		border: none;
		cursor: pointer;
		color: var(--color-muted);
		padding: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		font-size: 16px;
		line-height: 1;
	}

	.dismiss-button:hover {
		color: var(--color-fg);
	}
</style>
