<script lang="ts">
	/**
	 * The in-file search bar: a text input and a "match X of Y" / "No matches"
	 * indicator, plus previous/next/close controls. Mounted in App.svelte's
	 * content-area when search is open. Theme tokens only, mirroring the
	 * explorer Toolbar's filter-input house style (focus-within accent glow).
	 * Enter/Shift+Enter/Escape are NOT handled here — App.svelte's window-level
	 * keydown listener (via viewerKeymap's resolveViewerShortcut) owns them, so
	 * they work the same whether or not this input currently has focus.
	 */
	interface Props {
		query: string;
		totalMatches: number;
		/** Zero-based index of the current match, or null when there isn't one. */
		currentMatchIndex: number | null;
		onQueryInput: (value: string) => void;
		onNext: () => void;
		onPrevious: () => void;
		onClose: () => void;
		/**
		 * Incremented by the caller whenever the input should be focused and
		 * its text selected (for example re-opening Ctrl+F while already
		 * open). Only the CHANGE matters, so the initial mount never steals
		 * focus from whatever had it before the bar opened.
		 */
		focusSignal?: number;
	}

	let {
		query,
		totalMatches,
		currentMatchIndex,
		onQueryInput,
		onNext,
		onPrevious,
		onClose,
		focusSignal = 0,
	}: Props = $props();

	let indicatorText = $derived.by(() => {
		if (!query) return '';
		if (totalMatches === 0) return 'No matches';
		return `${(currentMatchIndex ?? 0) + 1} of ${totalMatches}`;
	});

	let inputElement = $state<HTMLInputElement | null>(null);

	// Unlike the explorer Toolbar's filter input (always mounted from app
	// start, so its focusSignal effect skips the initial run to avoid
	// stealing focus merely because the app happened to load), SearchBar is
	// only ever mounted BY App.svelte opening search — every mount
	// corresponds to the user just having pressed Ctrl+F, which should
	// always take focus. So this focuses on every run, including the first:
	// both "just opened" (mount) and "Ctrl+F again while already open"
	// (focusSignal change while mounted) are real focus requests here.
	$effect(() => {
		focusSignal;
		inputElement?.focus();
		inputElement?.select();
	});

	function handleInput(event: Event): void {
		onQueryInput((event.currentTarget as HTMLInputElement).value);
	}
</script>

<div class="search-bar">
	<input
		class="search-input"
		type="text"
		placeholder="Find in file..."
		value={query}
		bind:this={inputElement}
		oninput={handleInput}
	/>
	<span class="match-indicator">{indicatorText}</span>
	<button
		type="button"
		class="icon-btn search-previous"
		title="Previous match (Shift+Enter)"
		aria-label="Previous match"
		disabled={totalMatches === 0}
		onclick={onPrevious}
	>
		&#9650;
	</button>
	<button
		type="button"
		class="icon-btn search-next"
		title="Next match (Enter)"
		aria-label="Next match"
		disabled={totalMatches === 0}
		onclick={onNext}
	>
		&#9660;
	</button>
	<button
		type="button"
		class="icon-btn search-close"
		title="Close search (Escape)"
		aria-label="Close search"
		onclick={onClose}
	>
		&#10005;
	</button>
</div>

<style>
	.search-bar {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 38px;
		flex: none;
		padding: 0 10px;
		border-bottom: 1px solid var(--color-border);
		background: var(--color-bg-alt);
	}

	.search-input {
		flex: 1;
		min-width: 0;
		max-width: 320px;
		height: 28px;
		padding: 0 8px;
		background: var(--color-bg);
		border: 1px solid var(--color-border);
		border-radius: 6px;
		color: var(--color-fg);
		font-size: 13px;
		outline: none;
		transition:
			border-color 120ms ease,
			box-shadow 120ms ease;
	}

	.search-input:focus-visible,
	.search-input:focus {
		border-color: var(--color-accent);
		box-shadow:
			0 0 0 1px var(--color-accent),
			0 0 10px color-mix(in oklab, var(--color-accent) 25%, transparent);
	}

	.match-indicator {
		font-size: 12px;
		color: var(--color-muted);
		min-width: 70px;
		flex: none;
	}

	.icon-btn {
		width: 24px;
		height: 24px;
		flex: none;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		background: transparent;
		border: none;
		border-radius: 6px;
		color: var(--color-fg-alt);
		cursor: pointer;
		font-size: 10px;
	}

	.icon-btn:hover:not(:disabled) {
		background: var(--color-surface-hover, hsla(230, 14%, 42%, 1));
	}

	.icon-btn:focus-visible {
		outline: 2px solid var(--color-accent);
		outline-offset: 1px;
	}

	.icon-btn:disabled {
		opacity: 0.35;
		cursor: default;
	}
</style>
