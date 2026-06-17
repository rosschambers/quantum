<script lang="ts">
  import type { Timer } from "@quantum/client";
  import {
    remainingFraction,
    displayFraction,
    rampColor,
    formatTime,
  } from "./fraction";

  /** Concrete styles a `mixed` timer cycles through, in playground order. */
  const STYLES = ["ring", "wedge", "pie", "dots", "bar"] as const;

  /**
   * Per-timer total-duration approximation.
   *
   * The backend sends only a fire time, never the original total. We record
   * the largest remaining span ever seen for each timer id: on first sight
   * that span is the full duration (so a depleting ring starts full), and a
   * later edit that extends the timer grows the recorded total to match. The
   * map lives for the page's lifetime; a timer first observed mid-countdown
   * therefore appears full and depletes from there. Expired timers are not
   * recorded because they render a fixed end-state regardless of total.
   */
  const observedTotals = new Map<string, number>();

  function trackTotal(id: string, remaining: number): number {
    const previous = observedTotals.get(id) ?? 0;
    const total = Math.max(previous, remaining, 1);
    observedTotals.set(id, total);
    return total;
  }

  function lerp(a: number, b: number, t: number): number {
    return a + (b - a) * t;
  }

  let {
    timer,
    nowUnix,
    indexInList,
  }: { timer: Timer; nowUnix: number; indexInList: number } = $props();

  const visual = $derived(timer.visual);
  const notify = $derived(timer.notify);

  const firesAt = $derived(
    timer.kind.type === "one_shot"
      ? timer.kind.end_unix
      : timer.kind.next_fire_unix,
  );
  const remaining = $derived(firesAt - nowUnix);
  const isExpired = $derived(timer.status === "expired");

  const frac = $derived.by(() => {
    if (isExpired) return 0;
    const total = trackTotal(timer.id, remaining);
    return remainingFraction(nowUnix, firesAt, total);
  });
  const displayFrac = $derived(displayFraction(frac, visual.fill));

  const styleId = $derived(
    visual.style === "mixed"
      ? STYLES[indexInList % STYLES.length]
      : visual.style,
  );

  const stroke = $derived(
    notify.urgency_ramp
      ? rampColor(frac, visual.accent_hue, notify.ramp_threshold)
      : `hsl(${visual.accent_hue} 70% 60%)`,
  );
  const trackCol = $derived(
    `hsl(${visual.accent_hue} 30% 50% / ${visual.track_opacity / 100})`,
  );
  const inRamp = $derived(
    notify.urgency_ramp && frac <= notify.ramp_threshold / 100,
  );

  // Pulse while in the urgency ramp; flash near/at completion. Both are driven
  // off nowUnix (seconds) so they animate with the parent's rAF loop.
  const scale = $derived(
    inRamp && notify.pulse
      ? lerp(0.97, 1.05, (Math.sin(nowUnix * 5) + 1) / 2)
      : 1,
  );
  const opacity = $derived(
    frac <= 0.001 && notify.flash
      ? ((Math.sin(nowUnix * 9) + 1) / 2) * 0.7 + 0.3
      : 1,
  );

  // Geometry.
  const size = $derived(visual.size);
  const thick = $derived(visual.thickness);
  const center = $derived(size / 2);
  const radius = $derived((size - thick) / 2);
  const circumference = $derived(2 * Math.PI * radius);
  const dashOffset = $derived(circumference * (1 - displayFrac));
  const angle = $derived(displayFrac * 360);
  const innerR = $derived(size / 2 - thick);
  const wedgeMask = $derived(
    `radial-gradient(circle at center, transparent ${innerR}px, #000 ${innerR}px)`,
  );
  const barWidth = $derived(size * 1.4);
  const barHeight = $derived(Math.max(thick, 14));
  const dotCount = 12;
  const dotSize = $derived(Math.max(8, thick * 1.2));
  const dotsLit = $derived(Math.round(displayFrac * dotCount));
  const dots = $derived(
    Array.from({ length: dotCount }, (_, i) => i < dotsLit),
  );

  // Text.
  const timeText = $derived(
    formatTime(remaining, visual.time_format, frac),
  );
  const textColor = $derived.by(() => {
    if (visual.text_color === "accent") return stroke;
    if (visual.text_color === "muted") return "rgba(255,255,255,.62)";
    return "#fff";
  });
  const labelFont = $derived(
    Math.max(10, size * 0.12 * (visual.font_scale / 100)),
  );
  const timeFont = $derived(
    Math.max(10, size * 0.135 * (visual.font_scale / 100)),
  );

  function capClass(vis: string): string {
    return vis === "hover" ? "cap hoveronly" : "cap";
  }
</script>

{#snippet cap(text: string, isTime: boolean)}
  {@const vis = isTime ? visual.time_visibility : visual.label_visibility}
  {#if vis !== "hidden"}
    <div
      class={capClass(vis)}
      style="font-family:{isTime
        ? 'var(--font-mono, ui-monospace, monospace)'
        : 'var(--font-sans, system-ui, sans-serif)'};font-size:{isTime
        ? timeFont
        : labelFont}px;font-weight:{visual.font_weight};color:{textColor};{!isTime &&
      visual.uppercase
        ? 'text-transform:uppercase;letter-spacing:.5px;'
        : ''}{isTime ? 'opacity:.85;' : ''}"
    >
      {text}
    </div>
  {/if}
{/snippet}

{#snippet visualBody()}
  {#if styleId === "ring"}
    <svg width={size} height={size} viewBox="0 0 {size} {size}">
      <circle
        cx={center}
        cy={center}
        r={radius}
        fill="none"
        stroke={trackCol}
        stroke-width={thick}
      />
      <circle
        cx={center}
        cy={center}
        r={radius}
        fill="none"
        stroke={stroke}
        stroke-width={thick}
        stroke-linecap="round"
        stroke-dasharray={circumference}
        stroke-dashoffset={dashOffset}
        transform="rotate(-90 {center} {center})"
      />
    </svg>
  {:else if styleId === "wedge"}
    <div
      style="width:{size}px;height:{size}px;border-radius:50%;background:conic-gradient({stroke} {angle}deg, {trackCol} {angle}deg);-webkit-mask:{wedgeMask};mask:{wedgeMask};"
    ></div>
  {:else if styleId === "pie"}
    <div
      style="width:{size}px;height:{size}px;border-radius:50%;background:conic-gradient({stroke} {angle}deg, {trackCol} {angle}deg);"
    ></div>
  {:else if styleId === "bar"}
    <div
      style="width:{barWidth}px;height:{barHeight}px;border-radius:{barHeight}px;background:{trackCol};overflow:hidden;"
    >
      <div
        style="height:100%;width:{displayFrac *
          100}%;background:{stroke};border-radius:{barHeight}px;"
      ></div>
    </div>
  {:else if styleId === "dots"}
    <div
      style="width:{size}px;display:flex;flex-wrap:wrap;gap:{dotSize *
        0.6}px;justify-content:center;"
    >
      {#each dots as lit}
        <span
          style="width:{dotSize}px;height:{dotSize}px;border-radius:50%;display:inline-block;background:{lit
            ? stroke
            : trackCol};"
        ></span>
      {/each}
    </div>
  {/if}
{/snippet}

{#snippet visualWrapped()}
  {#if visual.reverse}
    <div style="display:inline-flex;transform:scaleX(-1);">
      {@render visualBody()}
    </div>
  {:else}
    {@render visualBody()}
  {/if}
{/snippet}

<div
  class="timer-visual"
  style="transform:scale({scale.toFixed(3)});opacity:{opacity.toFixed(2)};"
>
  {#if visual.text_position === "center"}
    <div class="center-wrap">
      {@render visualWrapped()}
      <div class="center-text">
        {@render cap(timer.label, false)}
        {@render cap(timeText, true)}
      </div>
    </div>
  {:else if visual.text_position === "above"}
    {@render cap(timer.label, false)}
    {@render cap(timeText, true)}
    {@render visualWrapped()}
  {:else}
    {@render visualWrapped()}
    {@render cap(timer.label, false)}
    {@render cap(timeText, true)}
  {/if}
</div>

<style>
  .timer-visual {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    transform-origin: center;
  }
  .center-wrap {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .center-text {
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }
  .cap {
    text-align: center;
    line-height: 1.2;
    white-space: nowrap;
  }
  .cap.hoveronly {
    opacity: 0;
    transition: opacity 0.15s;
  }
  .timer-visual:hover .cap.hoveronly {
    opacity: 1;
  }
</style>
