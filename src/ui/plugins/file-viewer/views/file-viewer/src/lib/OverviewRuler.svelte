<script lang="ts">
	/**
	 * The shared overview ruler: a 10px strip beside a renderer's scroll pane
	 * showing search-match marks (every renderer) and diff change marks (diff
	 * mode) on the same track, with a proportional scroll thumb beneath them.
	 * The strip itself is always reserved (so toggling search never reflows
	 * the renderer sideways); the marks and thumb render only once there is
	 * at least one mark, per the design's "drawn only when it has marks".
	 *
	 * All geometry is pure math from `overviewRuler.ts` — this component only
	 * measures its own height (the track) and the scroll element's scroll
	 * state, and wires the DOM events (scroll, resize, click) around it.
	 */
	import { layoutRulerSegments, thumbGeometry, scrollTopForTrackPosition, hitTestMark, type RulerMark } from './overviewRuler';
	import { observeLayout, coalesceToAnimationFrame } from './measureOffsets';

	interface Props {
		marks: readonly RulerMark[];
		scrollElement: HTMLElement | null | undefined;
		onMarkActivate?: (mark: RulerMark) => void;
		hitTolerance?: number;
	}

	let { marks, scrollElement, onMarkActivate, hitTolerance = 4 }: Props = $props();

	let rulerElement: HTMLDivElement | undefined = $state(undefined);
	let trackHeight = $state(0);
	let thumb: { top: number; height: number } = $state({ top: 0, height: 0 });

	let segments = $derived(marks.length > 0 ? layoutRulerSegments(marks, trackHeight) : []);

	function measure(): void {
		trackHeight = rulerElement?.clientHeight ?? 0;
		if (!scrollElement) {
			return;
		}
		thumb = thumbGeometry(
			{ scrollTop: scrollElement.scrollTop, clientHeight: scrollElement.clientHeight, scrollHeight: scrollElement.scrollHeight },
			trackHeight,
		);
	}

	$effect(() => {
		const ruler = rulerElement;
		const element = scrollElement;
		if (!ruler) {
			return;
		}

		measure();

		if (!element) {
			return;
		}

		// Scroll fires far more often than layout changes, so it shares the
		// same per-animation-frame coalescing helper as the resize path
		// (`observeLayout`) rather than measuring synchronously on every event.
		const scrollFrame = coalesceToAnimationFrame(measure);
		const handleScroll = () => scrollFrame.schedule();
		element.addEventListener('scroll', handleScroll, { passive: true });
		const stopRulerObserver = observeLayout(ruler, measure);
		const stopScrollObserver = observeLayout(element, measure);

		return () => {
			element.removeEventListener('scroll', handleScroll);
			scrollFrame.cancel();
			stopRulerObserver();
			stopScrollObserver();
		};
	});

	function handlePointerDown(event: PointerEvent): void {
		// Keeps focus in the search input: a bare click on the ruler must
		// never steal focus away from Ctrl+F's text field.
		event.preventDefault();
	}

	function handleClick(event: MouseEvent): void {
		if (!rulerElement) {
			return;
		}
		const rect = rulerElement.getBoundingClientRect();
		const y = event.clientY - rect.top;
		// Hit-testing walks the raw `marks`, never the merged `segments` drawn
		// above: a merged tick on screen can represent several real marks, and
		// testing the merged segment's bounds alone would lose which one the
		// click was closest to, so we always resolve to the nearest real mark.
		const hit = hitTestMark(marks, y, trackHeight, hitTolerance);
		if (hit) {
			onMarkActivate?.(hit);
			return;
		}
		if (!scrollElement) {
			return;
		}
		scrollElement.scrollTop = scrollTopForTrackPosition(y, trackHeight, {
			scrollTop: scrollElement.scrollTop,
			clientHeight: scrollElement.clientHeight,
			scrollHeight: scrollElement.scrollHeight,
		});
	}
</script>

<div
	class="overview-ruler"
	aria-hidden="true"
	bind:this={rulerElement}
	onpointerdown={handlePointerDown}
	onclick={handleClick}
>
	{#if marks.length > 0}
		<div class="ruler-thumb" style={`top: ${thumb.top}px; height: ${thumb.height}px;`}></div>
		{#each segments as segment, index (index)}
			<div
				class={`ruler-mark kind-${segment.kind} lane-${segment.lane}`}
				style={`top: ${segment.top}px; height: ${segment.height}px;`}
			></div>
		{/each}
	{/if}
</div>

<style>
	.overview-ruler {
		position: relative;
		width: 10px;
		flex: none;
		background: transparent;
		border-left: 1px solid var(--color-border);
		cursor: pointer;
	}

	.ruler-mark {
		position: absolute;
		border-radius: 1px;
	}

	.ruler-mark.lane-full {
		left: 1.5px;
		right: 1.5px;
	}

	.ruler-mark.lane-change {
		left: 0;
		width: 3px;
	}

	.ruler-mark.lane-search {
		right: 0;
		width: 3px;
	}

	.ruler-mark.kind-match {
		background: color-mix(in oklab, var(--color-fg) 65%, transparent);
	}

	.ruler-mark.kind-current-match {
		background: var(--color-fg);
		box-shadow: 0 0 0 1px var(--color-bg);
	}

	.ruler-mark.kind-added {
		background: var(--color-accent);
	}

	.ruler-mark.kind-removed {
		background: var(--color-error, #e5484d);
	}

	.ruler-mark.kind-mixed {
		background: linear-gradient(to right, var(--color-accent) 50%, var(--color-error, #e5484d) 50%);
	}

	.ruler-thumb {
		position: absolute;
		left: 0;
		right: 0;
		background: color-mix(in oklab, var(--color-fg) 12%, transparent);
		pointer-events: none;
	}
</style>
