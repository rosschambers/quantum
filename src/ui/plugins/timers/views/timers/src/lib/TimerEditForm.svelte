<script lang="ts">
  import type {
    Timer,
    VisualConfig,
    VisualStyle,
    Client,
  } from "@quantum/client";

  /** The styles offered in the picker. `mixed` is intentionally excluded. */
  const STYLES: VisualStyle[] = [
    "ring",
    "wedge",
    "pie",
    "dots",
    "bar",
    "spiral",
    "pulse",
  ];

  let {
    timer,
    client,
    nowUnix,
    onClose,
  }: {
    timer: Timer;
    client: Pick<Client, "call">;
    nowUnix: number;
    onClose: () => void;
  } = $props();

  // Initial values are captured once at mount so a ticking `nowUnix` does not
  // re-derive the editable fields out from under the user. The form is always
  // mounted fresh for a single timer (the scrim blocks reaching another
  // timer's edit button), so the one-time capture is the intended behaviour.
  // svelte-ignore state_referenced_locally
  const initial = (() => {
    const kind = timer.kind;
    return {
      label: timer.label,
      isOneShot: kind.type === "one_shot",
      minutes:
        kind.type === "one_shot"
          ? Math.max(0, Math.round(kind.end_unix - nowUnix) / 60)
          : 0,
      time: kind.type === "recurring" ? kind.time : { hour: 0, minute: 0 },
      style: timer.visual.style,
      accentHue: timer.visual.accent_hue,
    };
  })();

  const isOneShot = initial.isOneShot;

  let label = $state(initial.label);
  let minutes = $state(Math.round(initial.minutes));
  let timeText = $state(
    `${String(initial.time.hour).padStart(2, "0")}:${String(
      initial.time.minute,
    ).padStart(2, "0")}`,
  );
  let style = $state<VisualStyle>(initial.style);
  let accentMode = $state<"theme" | "custom">(
    initial.accentHue === null ? "theme" : "custom",
  );
  let accentHue = $state(initial.accentHue ?? 220);

  function parseTime(value: string): { hour: number; minute: number } {
    const [hourPart, minutePart] = value.split(":");
    return {
      hour: Number(hourPart) || 0,
      minute: Number(minutePart) || 0,
    };
  }

  function buildChanges(): Record<string, unknown> {
    const changes: Record<string, unknown> = {};

    if (label !== initial.label) {
      changes.label = label;
    }

    if (isOneShot) {
      if (Math.round(minutes) !== Math.round(initial.minutes)) {
        changes.duration_secs = Math.round(minutes * 60);
      }
    } else {
      const time = parseTime(timeText);
      if (
        time.hour !== initial.time.hour ||
        time.minute !== initial.time.minute
      ) {
        changes.time = time;
      }
    }

    const chosenAccentHue = accentMode === "theme" ? null : accentHue;
    if (style !== initial.style || chosenAccentHue !== initial.accentHue) {
      const visual: VisualConfig = {
        ...timer.visual,
        style,
        accent_hue: chosenAccentHue,
      };
      changes.visual = visual;
    }

    return changes;
  }

  function save(): void {
    const changes = buildChanges();
    client
      .call("timer.edit", { id: timer.id, changes })
      .catch((error: unknown) => {
        console.error("timer.edit failed", error);
      });
    onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      onClose();
    }
  }

  function handleScrimClick(event: MouseEvent): void {
    if (event.target === event.currentTarget) {
      onClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="scrim" onclick={handleScrimClick}>
  <div class="card">
    <div class="head">Edit timer</div>
    <div class="body">
      <div class="field">
        <label for="timer-edit-name">Name</label>
        <input id="timer-edit-name" type="text" bind:value={label} />
      </div>

      {#if isOneShot}
        <div class="field">
          <label for="timer-edit-duration">Duration (minutes)</label>
          <input
            id="timer-edit-duration"
            type="number"
            min="0"
            max="600"
            bind:value={minutes}
          />
        </div>
      {:else}
        <div class="field">
          <label for="timer-edit-time">Time</label>
          <input id="timer-edit-time" type="time" bind:value={timeText} />
        </div>
      {/if}

      <div class="field">
        <label for="timer-edit-style">Visual style</label>
        <select id="timer-edit-style" bind:value={style}>
          {#each STYLES as option}
            <option value={option}>{option}</option>
          {/each}
        </select>
      </div>

      <div class="field">
        <span class="field-label">Accent</span>
        <div class="accent-row">
          <button
            type="button"
            class="segment"
            class:active={accentMode === "theme"}
            onclick={() => (accentMode = "theme")}
          >
            Theme
          </button>
          <button
            type="button"
            class="segment"
            class:active={accentMode === "custom"}
            onclick={() => (accentMode = "custom")}
          >
            Custom
          </button>
          {#if accentMode === "custom"}
            <input
              type="number"
              class="hue"
              min="0"
              max="360"
              aria-label="Accent hue"
              bind:value={accentHue}
            />
          {/if}
        </div>
      </div>
    </div>

    <div class="foot">
      <button type="button" class="cancel" onclick={() => onClose()}>
        Cancel
      </button>
      <button type="button" class="save" onclick={save}>Save</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .card {
    width: 320px;
    background: var(--color-bg-alt, #181825);
    border: 1px solid var(--color-surface, #45475a);
    border-radius: 16px;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.6);
    overflow: hidden;
  }
  .head {
    padding: 14px 18px;
    border-bottom: 1px solid var(--color-surface, #313244);
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 1px;
    color: var(--color-fg-alt, #a6adc8);
  }
  .body {
    padding: 16px 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .field label,
  .field .field-label {
    display: block;
    font-size: 11px;
    color: var(--color-fg-alt, #a6adc8);
    margin-bottom: 6px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .field input[type="text"],
  .field input[type="number"],
  .field input[type="time"],
  .field select {
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid var(--color-surface, #45475a);
    background: var(--color-bg, #1e1e2e);
    color: var(--color-fg, #cdd6f4);
    font-family: inherit;
    font-size: 13px;
  }
  .accent-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .segment {
    flex: 1 1 auto;
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid var(--color-surface, #45475a);
    background: var(--color-bg, #1e1e2e);
    color: var(--color-fg-alt, #a6adc8);
    cursor: pointer;
    font-family: inherit;
    font-size: 12px;
  }
  .segment.active {
    background: var(--color-accent, #f38ba8);
    border-color: var(--color-accent, #f38ba8);
    color: var(--color-crust, #11111b);
    font-weight: 600;
  }
  .hue {
    width: 72px;
    flex: 0 0 auto;
  }
  .foot {
    padding: 12px 18px;
    border-top: 1px solid var(--color-surface, #313244);
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .foot button {
    padding: 8px 16px;
    border-radius: 8px;
    font-family: inherit;
    font-size: 12px;
    cursor: pointer;
    border: 1px solid var(--color-surface, #45475a);
    background: transparent;
    color: var(--color-fg-alt, #a6adc8);
  }
  .foot button.save {
    background: var(--color-accent, #f38ba8);
    border-color: var(--color-accent, #f38ba8);
    color: var(--color-crust, #11111b);
    font-weight: 600;
  }
</style>
