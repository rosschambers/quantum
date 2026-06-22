<script lang="ts">
  import {
    createClient,
    createTimerStore,
    openContextMenu,
    type Timer,
    type TimerSettings,
    type Point,
  } from "@quantum/client";
  import TimerVisual from "./lib/TimerVisual.svelte";
  import TimerEditForm from "./lib/TimerEditForm.svelte";
  import { defaultScatterPosition, clampScatterPosition } from "./lib/layout";

  const client = createClient();

  let timers: Timer[] = $state([]);
  let settings: TimerSettings | null = $state(null);
  let nowUnix = $state(Date.now() / 1000);
  let editingId: string | null = $state(null);

  const editingTimer = $derived(
    editingId === null ? null : (timers.find((t) => t.id === editingId) ?? null),
  );

  function dismiss(id: string): void {
    client.call("timer.dismiss", { id }).catch((error: unknown) => {
      console.error("timer.dismiss failed", error);
    });
  }

  function cancel(id: string): void {
    client.call("timer.cancel", { id }).catch((error: unknown) => {
      console.error("timer.cancel failed", error);
    });
  }

  function dismissAll(): void {
    client.call("timer.dismiss_all", {}).catch((error: unknown) => {
      console.error("timer.dismiss_all failed", error);
    });
  }

  function openCreate(): void {
    // Match TimerIndicator: pin the create overlay to this bar's monitor by
    // appending the per-monitor `@<connector>` suffix when present.
    const monitor = (window as unknown as { __quantum_monitor?: string })
      .__quantum_monitor;
    const name = monitor
      ? `plugin/timer-create/timer-create@${monitor}`
      : "plugin/timer-create/timer-create";
    client.call("view.toggle", { name }).catch((error: unknown) => {
      console.error(`view.toggle ${name} failed`, error);
    });
  }

  // Right-click a timer: quick actions for that specific timer.
  function timerMenu(event: MouseEvent, timer: Timer): void {
    openContextMenu(event, [
      { label: "Edit timer", onSelect: () => (editingId = timer.id) },
      { label: "Dismiss", onSelect: () => dismiss(timer.id) },
      { separator: true },
      { label: "Cancel timer", danger: true, onSelect: () => cancel(timer.id) },
    ]);
  }

  // Right-click the empty surface: surface-wide actions.
  function surfaceMenu(event: MouseEvent): void {
    openContextMenu(event, [
      { label: "New timer", onSelect: openCreate },
      {
        label: "Dismiss all",
        disabled: timers.length === 0,
        onSelect: dismissAll,
      },
    ]);
  }

  // In-flight scatter overrides keyed by timer id. During a drag we update
  // this immediately for responsiveness; on drop we persist via timer.edit
  // and the next snapshot carries the authoritative scatter_pos.
  let localScatter: Record<string, Point> = $state({});

  $effect(() => {
    const off = createTimerStore(client).subscribe((data) => {
      // Refresh "now" on every snapshot. The rAF loop below is gated off while
      // the timer list is empty, so without this `nowUnix` would stay frozen at
      // widget-load time; the first render of a newly created timer would then
      // compute a wildly inflated remaining span (uptime + duration), which
      // `trackTotal` would latch as the timer's total and corrupt the fill
      // fraction. Setting it here guarantees the first render uses current time.
      nowUnix = Date.now() / 1000;
      settings = data.settings;
      timers = data.timers;
    });
    return () => {
      off?.();
      client.close();
    };
  });

  // rAF loop ticks "now" so countdowns animate smoothly between snapshots.
  // Gate it on there being at least one timer: with an empty list there is
  // nothing to animate, so the loop must not run. Reading timers.length makes
  // this effect re-run when the first timer arrives (starting the loop) and
  // when the last timer leaves (cleanup cancels the pending frame).
  $effect(() => {
    if (timers.length === 0) return;
    let raf = 0;
    const loop = (): void => {
      nowUnix = Date.now() / 1000;
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  });

  const layout = $derived(settings?.layout ?? "scatter");
  const gap = $derived(settings?.gap ?? 24);
  const align = $derived(settings?.align ?? "top-left");

  function scatterPosFor(timer: Timer, index: number): Point {
    return (
      localScatter[timer.id] ??
      timer.scatter_pos ??
      defaultScatterPosition(index)
    );
  }

  const containerStyle = $derived.by(() => {
    const g = `${gap}px`;
    if (layout === "scatter") return "";
    const isCenter = align.includes("center");
    const [vertical, horizontal] = isCenter
      ? ["center", "center"]
      : align.split("-");
    if (layout === "row") {
      const items =
        vertical === "top"
          ? "flex-start"
          : vertical === "bottom"
            ? "flex-end"
            : "center";
      const justify =
        horizontal === "left"
          ? "flex-start"
          : horizontal === "right"
            ? "flex-end"
            : "center";
      return `gap:${g};align-items:${items};justify-content:${justify};`;
    }
    if (layout === "grid") {
      const justify =
        horizontal === "left"
          ? "start"
          : horizontal === "right"
            ? "end"
            : "center";
      return `gap:${g};justify-content:${justify};`;
    }
    // Default: vertical stack.
    const justify =
      vertical === "top"
        ? "flex-start"
        : vertical === "bottom"
          ? "flex-end"
          : "center";
    const items =
      horizontal === "left"
        ? "flex-start"
        : horizontal === "right"
          ? "flex-end"
          : "center";
    return `gap:${g};justify-content:${justify};align-items:${items};`;
  });

  // Svelte action: drag a scatter timer and persist its position on drop.
  function draggable(
    node: HTMLElement,
    params: { id: string; enabled: boolean },
  ): { update: (p: { id: string; enabled: boolean }) => void; destroy: () => void } {
    let id = params.id;
    let enabled = params.enabled;

    function onPointerDown(event: PointerEvent): void {
      if (!enabled) return;
      const stage = node.parentElement;
      if (!stage) return;
      const rect = stage.getBoundingClientRect();
      const startX = event.clientX;
      const startY = event.clientY;
      const originX = node.offsetLeft;
      const originY = node.offsetTop;
      node.setPointerCapture(event.pointerId);
      node.classList.add("dragging");

      function onMove(moveEvent: PointerEvent): void {
        const position = clampScatterPosition(
          originX + (moveEvent.clientX - startX),
          originY + (moveEvent.clientY - startY),
          rect.width,
          rect.height,
        );
        localScatter = { ...localScatter, [id]: position };
      }

      function onUp(): void {
        node.releasePointerCapture(event.pointerId);
        node.classList.remove("dragging");
        node.removeEventListener("pointermove", onMove);
        node.removeEventListener("pointerup", onUp);
        const position = localScatter[id] ?? { x: originX, y: originY };
        client
          .call("timer.edit", {
            id,
            changes: {
              scatter_pos: {
                x: Math.round(position.x),
                y: Math.round(position.y),
              },
            },
          })
          .catch(() => {});
      }

      node.addEventListener("pointermove", onMove);
      node.addEventListener("pointerup", onUp);
    }

    node.addEventListener("pointerdown", onPointerDown);
    return {
      update(p: { id: string; enabled: boolean }): void {
        id = p.id;
        enabled = p.enabled;
      },
      destroy(): void {
        node.removeEventListener("pointerdown", onPointerDown);
      },
    };
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="stage {layout}"
  style={containerStyle}
  oncontextmenu={surfaceMenu}
>
  {#each timers as timer, index (timer.id)}
    {#if layout === "scatter"}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="slot scatter-slot"
        style="left:{scatterPosFor(timer, index).x}px;top:{scatterPosFor(
          timer,
          index,
        ).y}px;"
        use:draggable={{ id: timer.id, enabled: true }}
        oncontextmenu={(event) => timerMenu(event, timer)}
      >
        <TimerVisual
          {timer}
          {nowUnix}
          indexInList={index}
          onDismiss={() => dismiss(timer.id)}
          onEdit={() => (editingId = timer.id)}
        />
      </div>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="slot" oncontextmenu={(event) => timerMenu(event, timer)}>
        <TimerVisual
          {timer}
          {nowUnix}
          indexInList={index}
          onDismiss={() => dismiss(timer.id)}
          onEdit={() => (editingId = timer.id)}
        />
      </div>
    {/if}
  {/each}
</div>

{#if editingTimer}
  <TimerEditForm
    timer={editingTimer}
    {client}
    {nowUnix}
    onClose={() => (editingId = null)}
  />
{/if}

<style>
  .stage {
    position: absolute;
    inset: 0;
    padding: 28px;
    box-sizing: border-box;
  }
  .stage.stack {
    display: flex;
    flex-direction: column;
  }
  .stage.row {
    display: flex;
    flex-direction: row;
    flex-wrap: wrap;
  }
  .stage.grid {
    display: grid;
    align-content: start;
    grid-template-columns: repeat(auto-fill, minmax(180px, max-content));
  }
  .stage.scatter {
    display: block;
  }
  .scatter-slot {
    position: absolute;
    cursor: grab;
    touch-action: none;
  }
  .scatter-slot:global(.dragging) {
    cursor: grabbing;
    z-index: 50;
  }
</style>
