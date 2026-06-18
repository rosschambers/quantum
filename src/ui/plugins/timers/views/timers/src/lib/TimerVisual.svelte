<script lang="ts">
  import type { Timer } from "@quantum/client";
  import {
    remainingFraction,
    displayFraction,
    rampColor,
    rampToWarning,
    resolveBaseColor,
    formatTime,
  } from "./fraction";

  /** Concrete styles a `mixed` timer cycles through, in playground order. */
  const STYLES = ["ring", "wedge", "pie", "dots", "bar", "spiral", "pulse"] as const;

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
    onDismiss,
    onEdit,
  }: {
    timer: Timer;
    nowUnix: number;
    indexInList: number;
    onDismiss?: () => void;
    onEdit?: () => void;
  } = $props();

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

  // The base colour ignores the urgency ramp: it follows the timer's accent
  // hue, or the theme accent when the hue is null. The fill-border accent and
  // the lighter gradient stop both derive from this stable base.
  const baseColor = $derived(resolveBaseColor(visual.accent_hue));

  // The progress colour adds the urgency ramp on top of the base. A custom hue
  // ramps toward red; the theme-driven base mixes toward the warning token.
  const stroke = $derived.by(() => {
    if (visual.accent_hue !== null) {
      return notify.urgency_ramp
        ? rampColor(frac, visual.accent_hue, notify.ramp_threshold)
        : baseColor;
    }
    return notify.urgency_ramp
      ? rampToWarning(frac, baseColor, notify.ramp_threshold)
      : baseColor;
  });

  // A faint track derived from the surface token; fully transparent at zero
  // opacity. `track_opacity` is a 0-100 percentage.
  const trackCol = $derived(
    visual.track_opacity === 0
      ? "transparent"
      : `color-mix(in oklab, transparent ${100 - visual.track_opacity}%, var(--color-surface))`,
  );

  // The outline colour that hugs the filling portion when `fill_border` is on.
  const fillBorderColor = $derived.by(() => {
    if (visual.fill_border_color === "dark") return "rgba(0,0,0,0.55)";
    if (visual.fill_border_color === "light") return "rgba(255,255,255,0.85)";
    return baseColor;
  });

  // A stable per-instance gradient id, sanitised for use in a `url(#...)`
  // reference. The list index keeps it unique even if two ids collide after
  // sanitisation.
  const gradientId = $derived(
    `timer-gradient-${indexInList}-${timer.id.replace(/[^a-zA-Z0-9_-]/g, "")}`,
  );

  // The paint applied to progress strokes and the pie fill: a gradient when
  // enabled, otherwise the solid progress colour.
  const strokePaint = $derived(
    visual.gradient_stroke ? `url(#${gradientId})` : stroke,
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

  // A point on a circle, measured in degrees clockwise from twelve o'clock.
  function pointOnCircle(
    centerX: number,
    centerY: number,
    pointRadius: number,
    degrees: number,
  ): [number, number] {
    const angleRadians = ((degrees - 90) * Math.PI) / 180;
    return [
      centerX + pointRadius * Math.cos(angleRadians),
      centerY + pointRadius * Math.sin(angleRadians),
    ];
  }

  // A filled pie sector from twelve o'clock, clockwise, covering `fraction`
  // of the disc. Returns an empty path for a vanishing slice and a closed
  // full-circle arc once the slice covers the whole disc.
  function sectorPath(
    centerX: number,
    centerY: number,
    sectorRadius: number,
    fraction: number,
  ): string {
    if (fraction >= 0.9999) {
      return `M ${centerX} ${centerY - sectorRadius} A ${sectorRadius} ${sectorRadius} 0 1 1 ${(
        centerX - 0.01
      ).toFixed(2)} ${centerY - sectorRadius} Z`;
    }
    if (fraction <= 0.0001) return "";
    const [startX, startY] = pointOnCircle(centerX, centerY, sectorRadius, 0);
    const [endX, endY] = pointOnCircle(
      centerX,
      centerY,
      sectorRadius,
      fraction * 360,
    );
    const largeArc = fraction > 0.5 ? 1 : 0;
    return `M ${centerX} ${centerY} L ${startX.toFixed(2)} ${startY.toFixed(
      2,
    )} A ${sectorRadius} ${sectorRadius} 0 ${largeArc} 1 ${endX.toFixed(
      2,
    )} ${endY.toFixed(2)} Z`;
  }

  // An Archimedean spiral that winds outward from the centre across `turns`
  // revolutions, drawn only for the leading `fraction` of its full length so
  // the visible arc grows with the displayed fraction.
  function spiralPath(
    centerX: number,
    centerY: number,
    maxRadius: number,
    fraction: number,
    turns: number,
  ): string {
    const steps = 120;
    const maxTheta = turns * 2 * Math.PI * fraction;
    let d = "";
    for (let i = 0; i <= steps; i++) {
      const theta = (i / steps) * maxTheta;
      const pointRadius = (theta / (turns * 2 * Math.PI)) * maxRadius;
      const x = centerX + pointRadius * Math.cos(theta - Math.PI / 2);
      const y = centerY + pointRadius * Math.sin(theta - Math.PI / 2);
      d += (i === 0 ? "M" : "L") + x.toFixed(1) + " " + y.toFixed(1) + " ";
    }
    return d;
  }

  // Geometry.
  const size = $derived(visual.size);
  const thick = $derived(visual.thickness);
  const center = $derived(size / 2);
  const radius = $derived((size - thick) / 2);
  const circumference = $derived(2 * Math.PI * radius);
  const dashOffset = $derived(circumference * (1 - displayFrac));
  // The border width that hugs ring and wedge progress arcs.
  const underlayWidth = $derived(thick + 2 * visual.fill_border_width);
  // The pie disc shrinks by the border width so the outline stays inside the
  // surface bounds, and its sector path follows the displayed fraction.
  const pieRadius = $derived(
    size / 2 - (visual.fill_border ? visual.fill_border_width : 0),
  );
  const sectorD = $derived(sectorPath(center, center, pieRadius, displayFrac));
  const barWidth = $derived(size * 1.4);
  const barHeight = $derived(Math.max(thick, 14));
  const dotCount = 12;
  const dotSize = $derived(Math.max(8, thick * 1.2));
  const dotsLit = $derived(Math.round(displayFrac * dotCount));
  const dots = $derived(
    Array.from({ length: dotCount }, (_, i) => i < dotsLit),
  );

  // The spiral winds 3.5 turns out to a radius one thickness short of the
  // surface edge. The track draws the full spiral; the progress path draws the
  // leading portion that grows with the displayed fraction.
  const spiralTurns = 3.5;
  const spiralMaxRadius = $derived(size / 2 - thick);
  const spiralTrackD = $derived(
    spiralPath(center, center, spiralMaxRadius, 1, spiralTurns),
  );
  const spiralProgressD = $derived(
    spiralPath(center, center, spiralMaxRadius, displayFrac, spiralTurns),
  );

  // The pulse beats faster as the timer nears zero. `nowUnix` is already in
  // seconds, so the sine phase uses it directly. The filled circle's radius and
  // opacity rise and fall with each beat.
  const pulseFrequencyHz = $derived(0.4 + (1 - frac) * 1.6);
  const pulseBeat = $derived(
    (Math.sin(nowUnix * pulseFrequencyHz * 2 * Math.PI) + 1) / 2,
  );
  const pulseTrackRadius = $derived(size / 2 - thick);
  const pulseInnerRadius = $derived(pulseTrackRadius * (0.82 + pulseBeat * 0.18));
  const pulseOpacity = $derived(0.35 + pulseBeat * 0.5);
  const pulseGlow = $derived(6 + pulseBeat * 22);

  // Text.
  const timeText = $derived(
    formatTime(remaining, visual.time_format, frac),
  );
  const textColor = $derived.by(() => {
    if (visual.text_color === "accent") return stroke;
    if (visual.text_color === "muted") return "var(--color-fg-alt)";
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

{#snippet gradientDefs()}
  {#if visual.gradient_stroke}
    <defs>
      <linearGradient id={gradientId} x1="0" y1="0" x2="1" y2="1">
        <stop offset="0%" stop-color="color-mix(in oklab, white 20%, {stroke})" />
        <stop offset="100%" stop-color={stroke} />
      </linearGradient>
    </defs>
  {/if}
{/snippet}

{#snippet visualBody()}
  {#if styleId === "ring" || styleId === "wedge"}
    {@const lineCap = styleId === "wedge" ? "butt" : "round"}
    <div class="circular" style="width:{size}px;height:{size}px;">
      <svg
        class="circular-svg"
        width={size}
        height={size}
        viewBox="0 0 {size} {size}"
      >
        {@render gradientDefs()}
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          stroke={trackCol}
          stroke-width={thick}
        />
        {#if visual.fill_border}
          <circle
            cx={center}
            cy={center}
            r={radius}
            fill="none"
            stroke={fillBorderColor}
            stroke-width={underlayWidth}
            stroke-linecap={lineCap}
            stroke-dasharray={circumference}
            stroke-dashoffset={dashOffset}
            transform="rotate(-90 {center} {center})"
          />
        {/if}
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          stroke={strokePaint}
          stroke-width={thick}
          stroke-linecap={lineCap}
          stroke-dasharray={circumference}
          stroke-dashoffset={dashOffset}
          transform="rotate(-90 {center} {center})"
        />
      </svg>
      {#if visual.depth_sheen}
        <div class="sheen"></div>
      {/if}
    </div>
  {:else if styleId === "pie"}
    <div class="circular" style="width:{size}px;height:{size}px;">
      <svg
        class="circular-svg"
        width={size}
        height={size}
        viewBox="0 0 {size} {size}"
      >
        {@render gradientDefs()}
        <circle cx={center} cy={center} r={pieRadius} fill={trackCol} />
        {#if sectorD}
          <path
            d={sectorD}
            fill={strokePaint}
            stroke={visual.fill_border ? fillBorderColor : "none"}
            stroke-width={visual.fill_border ? visual.fill_border_width : 0}
            stroke-linejoin="round"
          />
        {/if}
      </svg>
      {#if visual.depth_sheen}
        <div class="sheen"></div>
      {/if}
    </div>
  {:else if styleId === "bar"}
    <div
      style="width:{barWidth}px;height:{barHeight}px;border-radius:{barHeight}px;background:{trackCol};overflow:hidden;"
    >
      <div
        style="height:100%;width:{displayFrac *
          100}%;background:{stroke};border-radius:{barHeight}px;{visual.fill_border
          ? `box-shadow:inset 0 0 0 ${visual.fill_border_width}px ${fillBorderColor};`
          : ''}"
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
            : trackCol};{lit && visual.fill_border
            ? `box-shadow:inset 0 0 0 ${visual.fill_border_width}px ${fillBorderColor};`
            : ''}"
        ></span>
      {/each}
    </div>
  {:else if styleId === "spiral"}
    <div class="circular" style="width:{size}px;height:{size}px;">
      <svg
        class="circular-svg"
        width={size}
        height={size}
        viewBox="0 0 {size} {size}"
      >
        {@render gradientDefs()}
        <path
          d={spiralTrackD}
          fill="none"
          stroke={trackCol}
          stroke-width={thick}
          stroke-linecap="round"
        />
        {#if visual.fill_border}
          <path
            d={spiralProgressD}
            fill="none"
            stroke={fillBorderColor}
            stroke-width={underlayWidth}
            stroke-linecap="round"
          />
        {/if}
        <path
          d={spiralProgressD}
          fill="none"
          stroke={strokePaint}
          stroke-width={thick}
          stroke-linecap="round"
        />
      </svg>
      {#if visual.depth_sheen}
        <div class="sheen"></div>
      {/if}
    </div>
  {:else if styleId === "pulse"}
    <div class="circular" style="width:{size}px;height:{size}px;">
      <svg
        class="circular-svg"
        width={size}
        height={size}
        viewBox="0 0 {size} {size}"
      >
        <circle
          cx={center}
          cy={center}
          r={pulseTrackRadius}
          fill="none"
          stroke={trackCol}
          stroke-width="2"
        />
        <circle
          cx={center}
          cy={center}
          r={pulseInnerRadius}
          fill={stroke}
          opacity={pulseOpacity}
          stroke={visual.fill_border ? fillBorderColor : "none"}
          stroke-width={visual.fill_border ? visual.fill_border_width : 0}
          style="filter:drop-shadow(0 0 {pulseGlow}px {stroke});"
        />
      </svg>
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

  <div class="controls hoveronly">
    <button
      type="button"
      class="control"
      aria-label="Dismiss timer"
      onpointerdown={(event) => event.stopPropagation()}
      onclick={() => onDismiss?.()}
    >
      ✕
    </button>
    <button
      type="button"
      class="control"
      aria-label="Edit timer"
      onpointerdown={(event) => event.stopPropagation()}
      onclick={() => onEdit?.()}
    >
      ✎
    </button>
  </div>
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
  .circular {
    position: relative;
  }
  .circular-svg {
    position: absolute;
    inset: 0;
  }
  .sheen {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    pointer-events: none;
    background: radial-gradient(
      circle at 32% 26%,
      rgba(255, 255, 255, 0.28),
      rgba(255, 255, 255, 0.06) 38%,
      transparent 60%
    );
  }
  .cap {
    text-align: center;
    line-height: 1.2;
    white-space: nowrap;
    /* Permanent scrim so captions stay legible over any backdrop. */
    text-shadow: 0 1px 6px rgba(0, 0, 0, 0.7);
  }
  .cap.hoveronly {
    opacity: 0;
    transition: opacity 0.15s;
  }
  .timer-visual:hover .cap.hoveronly {
    opacity: 1;
  }
  .controls {
    display: flex;
    gap: 6px;
  }
  .controls.hoveronly {
    opacity: 0;
    transition: opacity 0.15s;
  }
  .timer-visual:hover .controls.hoveronly {
    opacity: 1;
  }
  .control {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--color-surface, rgba(255, 255, 255, 0.18));
    background: var(--color-bg-alt, rgba(24, 24, 37, 0.85));
    color: var(--color-fg, #cdd6f4);
    cursor: pointer;
    font-size: 13px;
    line-height: 1;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
  }
  .control:hover {
    border-color: var(--color-accent, #f38ba8);
    color: var(--color-accent, #f38ba8);
  }
</style>
