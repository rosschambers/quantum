<script lang="ts">
  import { tick, untrack } from 'svelte';

  interface Props {
    uri: string;
    filename: string;
  }

  const { uri, filename }: Props = $props();

  // "Fit" restates today's default object-fit:contain behavior as an
  // explicit, named mode. "Free" covers both Actual size and any zoom
  // level reached through the +/- controls; the literal pixel dimensions
  // for "free" are always naturalWidth/naturalHeight multiplied by the
  // same scale, so aspect ratio can never distort independently.
  type ZoomMode = 'fit' | 'free';

  const MINIMUM_SCALE = 0.1;
  const MAXIMUM_SCALE = 8;
  const ZOOM_STEP_FACTOR = 1.25;

  let zoomMode: ZoomMode = $state('fit');
  let scale = $state(1);
  let naturalWidth = $state(0);
  let naturalHeight = $state(0);
  let hasLoadError = $state(false);
  let loadErrorMessage = $state('');
  let isPanning = $state(false);

  let viewportElement: HTMLDivElement | undefined = $state(undefined);
  let imageElement: HTMLImageElement | undefined = $state(undefined);

  let activePointerId: number | null = null;
  let panViewportElement: HTMLDivElement | undefined;
  let panStartClientX = 0;
  let panStartClientY = 0;
  let panStartScrollLeft = 0;
  let panStartScrollTop = 0;

  // Reset every piece of image-specific state whenever the uri changes, so
  // stale zoom, pan, or error state from a previous image never leaks into
  // the next one opened in the same panel.
  $effect(() => {
    void uri;
    untrack(() => {
      resetPan();
      if (viewportElement) {
        viewportElement.scrollLeft = 0;
        viewportElement.scrollTop = 0;
      }
    });
    zoomMode = 'fit';
    scale = 1;
    naturalWidth = 0;
    naturalHeight = 0;
    hasLoadError = false;
    loadErrorMessage = '';
    return resetPan;
  });

  const displayWidth = $derived(zoomMode === 'fit' ? null : clampFinite(naturalWidth * scale));
  const displayHeight = $derived(zoomMode === 'fit' ? null : clampFinite(naturalHeight * scale));
  const zoomLabel = $derived(zoomMode === 'fit' ? 'Fit' : `${Math.round(scale * 100)}%`);
  const canZoomIn = $derived(zoomMode === 'fit' || scale < MAXIMUM_SCALE);
  const canZoomOut = $derived(zoomMode === 'free' && scale > MINIMUM_SCALE);

  function clampFinite(value: number): number {
    return Number.isFinite(value) ? value : 0;
  }

  function clampScale(nextScale: number): number {
    if (!Number.isFinite(nextScale)) return 1;
    return Math.min(MAXIMUM_SCALE, Math.max(MINIMUM_SCALE, nextScale));
  }

  // Anchors a zoom change on the content point currently centered in the
  // viewport (sensible default when there is no cursor position to anchor
  // on, such as the toolbar buttons or the keyboard shortcuts). Captured
  // BEFORE the scale changes, applied AFTER the DOM reflects the new size.
  function captureAnchor(): { x: number; y: number } | null {
    if (!viewportElement || !imageElement) return null;
    const viewportBounds = viewportElement.getBoundingClientRect();
    const imageBounds = imageElement.getBoundingClientRect();
    if (!imageBounds.width || !imageBounds.height) return null;
    return {
      x: (viewportBounds.left + viewportElement.clientLeft + viewportElement.clientWidth / 2 - imageBounds.left) * naturalWidth / imageBounds.width,
      y: (viewportBounds.top + viewportElement.clientTop + viewportElement.clientHeight / 2 - imageBounds.top) * naturalHeight / imageBounds.height,
    };
  }

  function applyAnchor(anchor: { x: number; y: number } | null): void {
    if (!anchor || !viewportElement || !imageElement || !naturalWidth || !naturalHeight) return;
    const viewportBounds = viewportElement.getBoundingClientRect();
    const imageBounds = imageElement.getBoundingClientRect();
    const scrollLeft = viewportElement.scrollLeft + imageBounds.left - viewportBounds.left - viewportElement.clientLeft
      + anchor.x * imageBounds.width / naturalWidth - viewportElement.clientWidth / 2;
    const scrollTop = viewportElement.scrollTop + imageBounds.top - viewportBounds.top - viewportElement.clientTop
      + anchor.y * imageBounds.height / naturalHeight - viewportElement.clientHeight / 2;
    viewportElement.scrollLeft = Math.max(0, Math.min(scrollLeft, viewportElement.scrollWidth - viewportElement.clientWidth));
    viewportElement.scrollTop = Math.max(0, Math.min(scrollTop, viewportElement.scrollHeight - viewportElement.clientHeight));
  }

  async function setZoom(nextMode: ZoomMode, nextScale: number): Promise<void> {
    const anchor = captureAnchor();
    resetPan();
    zoomMode = nextMode;
    scale = nextScale;
    await tick();
    applyAnchor(anchor);
  }

  function zoomIn(): void {
    if (zoomMode === 'fit') {
      void setZoom('free', 1);
      return;
    }
    void setZoom('free', clampScale(scale * ZOOM_STEP_FACTOR));
  }

  function zoomOut(): void {
    if (zoomMode === 'fit') return;
    void setZoom('free', clampScale(scale / ZOOM_STEP_FACTOR));
  }

  function setFit(): void {
    resetPan();
    zoomMode = 'fit';
    scale = 1;
    if (viewportElement) {
      viewportElement.scrollLeft = 0;
      viewportElement.scrollTop = 0;
    }
  }

  function setActualSize(): void {
    void setZoom('free', 1);
  }

  function handleImageLoad(): void {
    hasLoadError = false;
    loadErrorMessage = '';
    if (imageElement) {
      naturalWidth = imageElement.naturalWidth;
      naturalHeight = imageElement.naturalHeight;
    }
  }

  function handleImageError(): void {
    hasLoadError = true;
    loadErrorMessage = `Failed to load image: ${filename}`;
  }

  function isPannable(): boolean {
    if (!viewportElement || zoomMode !== 'free') return false;
    return (
      viewportElement.scrollWidth > viewportElement.clientWidth ||
      viewportElement.scrollHeight > viewportElement.clientHeight
    );
  }

  function handlePointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    if (!viewportElement || !isPannable()) return;
    isPanning = true;
    activePointerId = event.pointerId;
    panViewportElement = viewportElement;
    panStartClientX = event.clientX;
    panStartClientY = event.clientY;
    panStartScrollLeft = viewportElement.scrollLeft;
    panStartScrollTop = viewportElement.scrollTop;
    // jsdom (used under vitest) does not implement pointer capture; guard
    // so component behavior is identical under test and under the real
    // WebKitGTK webview, where it is supported.
    if (typeof viewportElement.setPointerCapture === 'function') {
      viewportElement.setPointerCapture(event.pointerId);
    }
  }

  function handlePointerMove(event: PointerEvent): void {
    if (!isPanning || activePointerId !== event.pointerId || !viewportElement) return;
    viewportElement.scrollLeft = panStartScrollLeft - (event.clientX - panStartClientX);
    viewportElement.scrollTop = panStartScrollTop - (event.clientY - panStartClientY);
  }

  function endPan(event: PointerEvent): void {
    if (activePointerId !== event.pointerId) return;
    resetPan();
  }

  function resetPan(): void {
    const pointerId = activePointerId;
    const captureElement = panViewportElement;
    isPanning = false;
    activePointerId = null;
    panViewportElement = undefined;
    panStartClientX = 0;
    panStartClientY = 0;
    panStartScrollLeft = 0;
    panStartScrollTop = 0;
    if (
      pointerId !== null && captureElement &&
      typeof captureElement.hasPointerCapture === 'function' &&
      typeof captureElement.releasePointerCapture === 'function' &&
      captureElement.hasPointerCapture(pointerId)
    ) {
      captureElement.releasePointerCapture(pointerId);
    }
  }

  // Local to the pan area only: these never reach the window-level keydown
  // listener App.svelte installs, so this component's own +/-/0 shortcuts
  // can never conflict with App's Escape/heading-navigation shortcuts.
  function handlePanAreaKeyDown(event: KeyboardEvent): void {
    if (event.key === '+' || event.key === '=') {
      event.preventDefault();
      event.stopPropagation();
      zoomIn();
    } else if (event.key === '-' || event.key === '_') {
      event.preventDefault();
      event.stopPropagation();
      zoomOut();
    } else if (event.key === '0') {
      event.preventDefault();
      event.stopPropagation();
      setFit();
    }
  }

  // Suppress the browser's native drag-ghost so pointer-based panning is
  // the only drag behavior the image exhibits.
  function suppressNativeDragGhost(event: DragEvent): void {
    event.preventDefault();
  }
</script>

<div class="image-renderer">
  {#if !hasLoadError}
    <div class="toolbar">
      <button
        type="button"
        class="mode-fit"
        class:active={zoomMode === 'fit'}
        title="Fit to window"
        aria-label="Fit image to window"
        onclick={setFit}
      >
        Fit
      </button>
      <button
        type="button"
        class="mode-actual"
        class:active={zoomMode === 'free' && scale === 1}
        title="Actual size (100%)"
        aria-label="Actual size"
        onclick={setActualSize}
      >
        1:1
      </button>
      <button type="button" title="Zoom out" aria-label="Zoom out" disabled={!canZoomOut} onclick={zoomOut}>
        &minus;
      </button>
      <span class="zoom-label">{zoomLabel}</span>
      <button type="button" title="Zoom in" aria-label="Zoom in" disabled={!canZoomIn} onclick={zoomIn}>
        +
      </button>
    </div>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="viewport"
    class:panning={isPanning}
    role="group"
    aria-label="Image pan area"
    tabindex="0"
    bind:this={viewportElement}
    onpointerdown={handlePointerDown}
    onpointermove={handlePointerMove}
    onpointerup={endPan}
    onpointercancel={endPan}
    onlostpointercapture={endPan}
    onkeydown={handlePanAreaKeyDown}
  >
    {#if hasLoadError}
      <div class="image-error">
        <p class="error-title">Error</p>
        <p class="error-message">{loadErrorMessage}</p>
      </div>
    {/if}
    <img
      class:fit={zoomMode === 'fit'}
      class:hidden={hasLoadError}
      style={zoomMode === 'fit' ? undefined : `width: ${displayWidth}px; height: ${displayHeight}px;`}
      bind:this={imageElement}
      src={uri}
      alt={filename}
      draggable="false"
      onload={handleImageLoad}
      onerror={handleImageError}
      ondragstart={suppressNativeDragGhost}
    />
  </div>
</div>

<style>
  .image-renderer {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--color-border, #ddd);
    background: var(--color-bg, #fff);
    flex-shrink: 0;
  }

  .toolbar button {
    background: transparent;
    border: none;
    color: var(--color-fg, #000);
    cursor: pointer;
    padding: 4px 8px;
    font-size: 13px;
    font-family: var(--font-mono, monospace);
    line-height: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    transition: background-color 200ms;
  }

  .toolbar button:hover:not(:disabled) {
    background: var(--color-surface, #eee);
  }

  .toolbar button:active:not(:disabled) {
    background: var(--color-border, #ddd);
  }

  .toolbar button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .toolbar button.active {
    color: var(--color-accent, #a6e3a1);
    background: color-mix(in oklab, var(--color-accent, #a6e3a1) 12%, transparent);
  }

  .zoom-label {
    min-width: 48px;
    text-align: center;
    font-size: 12px;
    font-family: var(--font-mono, monospace);
    color: var(--color-fg-alt, #666);
  }

  .viewport {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: flex-start;
    justify-content: flex-start;
    overflow: auto;
    padding: 32px;
    background: var(--color-bg-alt);
    cursor: default;
  }

  .viewport:focus-visible {
    outline: 2px solid var(--color-accent, #007bff);
    outline-offset: -2px;
  }

  .viewport.panning {
    cursor: grabbing;
  }

  img {
    object-fit: contain;
    flex-shrink: 0;
    /* Auto margins center spare space but resolve to zero on overflow. */
    margin: auto;
  }

  img.fit {
    max-width: 100%;
    max-height: 100%;
  }

  img.hidden {
    display: none;
  }

  .image-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
    padding: 32px;
  }

  .error-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--color-error, #dc2626);
  }

  .error-message {
    font-size: 14px;
    color: var(--color-fg-alt, #666);
    font-family: var(--font-mono, monospace);
    white-space: pre-wrap;
    max-width: 90%;
  }
</style>
