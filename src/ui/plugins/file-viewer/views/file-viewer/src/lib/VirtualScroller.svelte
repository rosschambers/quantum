<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		lines: string[];
		lineHeight: number;
		bufferLines?: number;
		children: Snippet<[
			{ visibleLines: string[]; visibleStart: number; visibleEnd: number }
		]>;
	}

	let {
		lines,
		lineHeight,
		bufferLines = 50,
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
	let offsetTop = $derived(visibleStart * lineHeight);

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
</script>

<div
	bind:this={container}
	class="virtual-scroller"
	style="overflow-y: auto; overflow-x: hidden; height: 100%; position: relative;"
>
	<div
		style="height: {lines.length * lineHeight}px; position: relative;"
	>
		<div
			style="transform: translateY({offsetTop}px); will-change: transform;"
		>
			{@render children({ visibleLines, visibleStart, visibleEnd })}
		</div>
	</div>
</div>

<style>
	.virtual-scroller {
		overflow-y: auto;
		overflow-x: hidden;
	}
</style>
