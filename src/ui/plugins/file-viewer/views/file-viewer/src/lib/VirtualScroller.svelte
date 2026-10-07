<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		lines: string[];
		lineHeight: number;
		bufferLines?: number;
		verticalPadding?: number;
		/**
		 * When set, scrolls the container to this line index from outside
		 * (used by search-result navigation to reveal an offscreen match).
		 * Change scrollRequest to repeat navigation to an unchanged line index.
		 * Existing user-scroll behavior is otherwise unaffected.
		 */
		scrollToIndex?: number;
		/** Identity or revision of a navigation request, even on the same line. */
		scrollRequest?: unknown;
		/**
		 * Bindable mirrors of the internally-computed visible window, for a
		 * caller that needs the current range outside the snippet (for
		 * example CodeRenderer's LineNumbers gutter). Writing to a bindable
		 * prop must happen inside an $effect, never as a side effect of a
		 * template expression (`{@const x = (externalVar = ...)}`) — that
		 * pattern throws `state_unsafe_mutation` the moment the write is
		 * triggered from inside a derived/template re-render rather than a
		 * user-initiated event, which `scrollToIndex`-driven programmatic
		 * scrolling does trigger.
		 */
		visibleStart?: number;
		visibleEnd?: number;
		children: Snippet<[
			{ visibleLines: string[]; visibleStart: number; visibleEnd: number; lineHeight: number; rowStyle: string }
		]>;
	}

	let {
		lines,
		lineHeight,
		bufferLines = 50,
		verticalPadding = 0,
		scrollToIndex,
		scrollRequest,
		visibleStart: externalVisibleStart = $bindable(undefined),
		visibleEnd: externalVisibleEnd = $bindable(undefined),
		children
	}: Props = $props();

	let container: HTMLDivElement | undefined = $state();
	let scrollTop = $state(0);
	let containerHeight = $state(0);

	let visibleStart = $derived(Math.max(0, Math.floor(scrollTop / lineHeight) - bufferLines));
	let visibleEnd = $derived(
		Math.min(
			lines.length,
			Math.ceil((scrollTop + containerHeight) / lineHeight) + bufferLines
		)
	);
	let visibleLines = $derived(lines.slice(visibleStart, visibleEnd));
	let offsetTop = $derived(verticalPadding + visibleStart * lineHeight);
	let totalHeight = $derived(lines.length * lineHeight + 2 * verticalPadding);
	let rowStyle = $derived(`display: block; height: ${lineHeight}px; line-height: ${lineHeight}px; box-sizing: border-box; white-space: pre;`);

	$effect(() => {
		externalVisibleStart = visibleStart;
		externalVisibleEnd = visibleEnd;
	});

	let rafId: number | null = null;

	function handleScroll(event: Event) {
		const target = event.target as HTMLDivElement;
		scrollTop = target.scrollTop;

		if (rafId !== null) {
			cancelAnimationFrame(rafId);
		}

		rafId = requestAnimationFrame(() => {
			rafId = null;
		});
	}

	function updateHeight() {
		if (container) {
			containerHeight = container.clientHeight;
		}
	}

	$effect(() => {
		const el = container;
		if (el) {
			el.addEventListener('scroll', handleScroll, { passive: true });
			updateHeight();
			const resizeObserver = new ResizeObserver(() => {
				updateHeight();
			});
			resizeObserver.observe(el);

			return () => {
				el.removeEventListener('scroll', handleScroll);
				resizeObserver.disconnect();
			};
		}
	});

	$effect(() => {
		void scrollRequest;
		if (scrollToIndex === undefined || !container) return;
		const maxScroll = Math.max(0, totalHeight - containerHeight);
		const target = Math.min(Math.max(0, scrollToIndex * lineHeight), maxScroll);
		scrollTop = target;
		container.scrollTop = target;
	});
</script>

<div
	bind:this={container}
	class="virtual-scroller"
	style="overflow: auto; height: 100%; position: relative; flex: 1; min-width: 0;"
>
	<div
		style="height: {totalHeight}px; position: relative;"
	>
		<div
			style="transform: translateY({offsetTop}px); will-change: transform;"
		>
			{@render children({ visibleLines, visibleStart, visibleEnd, lineHeight, rowStyle })}
		</div>
	</div>
</div>

<style>
	.virtual-scroller {
		overflow-y: auto;
		overflow-x: hidden;
	}
</style>
