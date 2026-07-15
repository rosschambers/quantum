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
  const VIEW = "plugin/find-mouse/find-mouse";

  const client = createClient();

  // Live cursor origin in viewport coordinates. The ripple container is
  // translated here so the rings stay centered on the pointer as it moves.
  let origin = $state({ x: 0, y: 0 });
  let hasPosition = $state(false);

  // References to the ring elements, populated via bind:this so each can be
  // driven directly with the Web Animations API.
  const ringElements: HTMLDivElement[] = [];
  const rings = computeRingSpecs(RING_COUNT, DURATION);

  // Guards against re-entrant flashes if the window's visibility toggles
  // faster than a single flash completes.
  let flashing = false;

  // Run one complete flash: start the gated cursor poll, follow the pointer
  // for the ripple's lifetime, then stop polling and hide the window. The
  // window is a reused (never-destroyed) layer-shell surface, so the Svelte
  // component mounts only once; every subsequent `view.show` re-runs this via
  // the Page Visibility API rather than a fresh mount.
  function startFlash(): void {
    if (flashing) return;
    flashing = true;
    hasPosition = false;
    let gotPosition = false;

    client.call(CURSOR_WATCH, {}).catch((error: unknown) => {
      console.error("cursor.watch failed", error);
    });

    const off = client.subscribe(CURSOR_EVENT_CHANNEL, (payload) => {
      const position = payload as CursorPosition;
      if (
        position &&
        typeof position.x === "number" &&
        typeof position.y === "number"
      ) {
        gotPosition = true;
        hasPosition = true;
        origin = { x: position.x, y: position.y };
      }
    });

    // Fallback: if no position has arrived shortly after the flash starts,
    // center the ripples so the flash is never invisible.
    const fallbackTimer = setTimeout(() => {
      if (!gotPosition) {
        origin = { x: window.innerWidth / 2, y: window.innerHeight / 2 };
        hasPosition = true;
      }
    }, 150);

    // Drive each ring's expansion, staggered by the computed ring specs.
    // Cancel any residual animation from a previous flash first so a rapid
    // re-show restarts cleanly.
    for (let index = 0; index < ringElements.length; index += 1) {
      const element = ringElements[index];
      if (!element) continue;
      for (const animation of element.getAnimations()) animation.cancel();
      element.animate(
        [
          { transform: "scale(0.15)", opacity: 0.9 },
          { transform: "scale(1)", opacity: 0 },
        ],
        {
          duration: DURATION,
          delay: rings[index].delayMs,
          easing: "cubic-bezier(.15,.6,.3,1)",
          // "both": hold the first keyframe during the stagger delay AND the
          // last (fully faded) keyframe after finishing, so a ring never snaps
          // back to its base solid style before the window hides.
          fill: "both",
        },
      );
    }

    // After the flash completes, stop streaming positions and hide this view.
    // Hiding unmaps the surface; the next `view.show` re-maps it and fires a
    // visibilitychange that starts the next flash.
    setTimeout(
      () => {
        clearTimeout(fallbackTimer);
        off();
        client.call(CURSOR_UNWATCH, {}).catch(() => {});
        client.call("view.hide", { name: VIEW }).catch(() => {});
        hasPosition = false;
        flashing = false;
      },
      DURATION + 120,
    );
  }

  $effect(() => {
    // The component mounts once, while the window is first shown, so flash
    // immediately. Thereafter each show re-maps the surface and the webview
    // reports itself visible again; re-arm on that transition.
    startFlash();

    const onVisibility = () => {
      if (document.visibilityState === "visible") startFlash();
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      client.close();
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
    border: 4px solid var(--color-accent, #7aa2f7);
    border-radius: 50%;
    /* Glow derived from the theme accent so the flash matches the active
       theme; color-mix adds the alpha the box-shadow needs. Falls back to the
       original blue when the token is absent. */
    box-shadow:
      0 0 10px color-mix(in srgb, var(--color-accent, #7aa2f7) 50%, transparent),
      0 0 24px color-mix(in srgb, var(--color-accent, #7aa2f7) 28%, transparent);
    box-sizing: border-box;
  }
</style>
