<script lang="ts">
  import {
    createClient,
    CURSOR_EVENT_CHANNEL,
    CURSOR_WATCH,
    CURSOR_UNWATCH,
    type CursorPosition,
  } from "@quantum/client";
  import { computeRingSpecs } from "./lib/ripples";

  // Baked visual parameters from the approved sonar-ripple design.
  const DURATION = 700;
  const RING_COUNT = 3;

  const client = createClient();

  // Live cursor origin in viewport coordinates. The ripple container is
  // translated here so the rings stay centered on the pointer as it moves.
  let origin = $state({ x: 0, y: 0 });
  let hasPosition = $state(false);

  // References to the ring elements, populated via bind:this so each can be
  // driven directly with the Web Animations API (matching the prototype).
  const ringElements: HTMLDivElement[] = [];

  const rings = computeRingSpecs(RING_COUNT, DURATION);

  $effect(() => {
    let cancelled = false;
    const timers: ReturnType<typeof setTimeout>[] = [];

    // Each incoming position updates the origin so the ripples follow the
    // cursor for the brief life of the flash.
    const off = client.subscribe(CURSOR_EVENT_CHANNEL, (payload) => {
      const position = payload as CursorPosition;
      if (
        position &&
        typeof position.x === "number" &&
        typeof position.y === "number"
      ) {
        hasPosition = true;
        origin = { x: position.x, y: position.y };
      }
    });

    client.call(CURSOR_WATCH, {}).catch((error: unknown) => {
      console.error("cursor.watch failed", error);
    });

    // Fallback: if no position has arrived shortly after mount, center the
    // ripples in the viewport so the flash is never invisible.
    const fallbackTimer = setTimeout(() => {
      if (!hasPosition) {
        origin = { x: window.innerWidth / 2, y: window.innerHeight / 2 };
        hasPosition = true;
      }
    }, 150);
    timers.push(fallbackTimer);

    // Drive each ring's expansion via the Web Animations API, staggered by the
    // computed ring specs to produce the outward sonar cadence.
    for (let index = 0; index < ringElements.length; index += 1) {
      const element = ringElements[index];
      if (!element) continue;
      element.animate(
        [
          { transform: "scale(0.15)", opacity: 0.9 },
          { transform: "scale(1)", opacity: 0 },
        ],
        {
          duration: DURATION,
          delay: rings[index].delayMs,
          easing: "cubic-bezier(.15,.6,.3,1)",
          fill: "backwards",
        },
      );
    }

    // After the flash completes, stop streaming positions and hide this view.
    const hideTimer = setTimeout(() => {
      void (async () => {
        off();
        try {
          await client.call(CURSOR_UNWATCH, {});
          await client.call("view.hide", {
            name: "plugin/find-mouse/find-mouse",
          });
        } catch (error: unknown) {
          console.error("find-mouse teardown failed", error);
        } finally {
          if (!cancelled) client.close();
        }
      })();
    }, DURATION + 120);
    timers.push(hideTimer);

    return () => {
      cancelled = true;
      for (const timer of timers) clearTimeout(timer);
      off();
    };
  });
</script>

<div
  class="ripples"
  style="transform: translate({origin.x}px, {origin.y}px); visibility: {hasPosition
    ? 'visible'
    : 'hidden'};"
>
  {#each rings as _ring, index (index)}
    <div class="ring" bind:this={ringElements[index]}></div>
  {/each}
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    /* Only the rings should paint; the surface is click-through at the
       compositor level, but stay transparent and non-interactive here too. */
    background: transparent;
    overflow: hidden;
    pointer-events: none;
  }

  :global(#app) {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  .ripples {
    position: absolute;
    top: 0;
    left: 0;
    pointer-events: none;
  }

  .ring {
    position: absolute;
    /* 100px diameter circle centered on the origin. */
    width: 100px;
    height: 100px;
    margin-left: -50px;
    margin-top: -50px;
    border: 4px solid #7aa2f7;
    border-radius: 50%;
    box-shadow:
      0 0 10px rgba(122, 162, 247, 0.5),
      0 0 24px rgba(122, 162, 247, 0.28);
    box-sizing: border-box;
  }
</style>
