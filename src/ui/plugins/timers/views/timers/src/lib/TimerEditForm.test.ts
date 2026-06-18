import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/svelte/svelte5";
import { tick } from "svelte";
import type {
  Timer,
  VisualConfig,
  NotifyConfig,
} from "@quantum/client";
import TimerEditForm from "./TimerEditForm.svelte";

const nowUnix = 1_000_000;

function defaultNotify(): NotifyConfig {
  return {
    notification: true,
    sound: "complete",
    urgency_ramp: true,
    ramp_threshold: 10,
    pulse: false,
    flash: false,
  };
}

function defaultVisual(): VisualConfig {
  return {
    style: "ring",
    size: 100,
    thickness: 20,
    fill: false,
    reverse: false,
    accent_hue: null,
    track_opacity: 0,
    label_visibility: "hover",
    time_visibility: "hover",
    text_position: "center",
    text_color: "muted",
    time_format: "clock",
    font_scale: 100,
    font_weight: 600,
    uppercase: true,
    gradient_stroke: true,
    fill_border: true,
    fill_border_width: 1,
    fill_border_color: "dark",
    depth_sheen: false,
  };
}

function makeOneShotTimer(): Timer {
  return {
    id: "timer-under-test",
    label: "Tea",
    kind: { type: "one_shot", end_unix: nowUnix + 300 },
    visual: defaultVisual(),
    notify: defaultNotify(),
    status: "active",
    scatter_pos: null,
  };
}

function makeMockClient() {
  const call = vi.fn(() => Promise.resolve(undefined));
  return { client: { call, subscribe: vi.fn(), close: vi.fn() }, call };
}

describe("TimerEditForm", () => {
  it("edits the name and saves only the changed label", async () => {
    const { client, call } = makeMockClient();
    const onClose = vi.fn();
    const { container } = render(TimerEditForm, {
      props: {
        timer: makeOneShotTimer(),
        client: client as never,
        nowUnix,
        onClose,
      },
    });

    const nameInput = container.querySelector(
      'input[type="text"]',
    ) as HTMLInputElement;
    expect(nameInput).not.toBeNull();
    expect(nameInput.value).toBe("Tea");
    await fireEvent.input(nameInput, { target: { value: "Coffee" } });

    const save = Array.from(container.querySelectorAll("button")).find((el) =>
      el.textContent?.includes("Save"),
    ) as HTMLButtonElement;
    expect(save).toBeTruthy();
    await fireEvent.click(save);
    await tick();

    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenCalledWith("timer.edit", {
      id: "timer-under-test",
      changes: { label: "Coffee" },
    });
  });

  it("maps a one-shot duration of ten minutes to duration_secs 600", async () => {
    const { client, call } = makeMockClient();
    const onClose = vi.fn();
    const { container } = render(TimerEditForm, {
      props: {
        timer: makeOneShotTimer(),
        client: client as never,
        nowUnix,
        onClose,
      },
    });

    const durationInput = container.querySelector(
      'input[type="number"]',
    ) as HTMLInputElement;
    expect(durationInput).not.toBeNull();
    await fireEvent.input(durationInput, { target: { value: "10" } });

    const save = Array.from(container.querySelectorAll("button")).find((el) =>
      el.textContent?.includes("Save"),
    ) as HTMLButtonElement;
    await fireEvent.click(save);
    await tick();

    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenCalledWith("timer.edit", {
      id: "timer-under-test",
      changes: { duration_secs: 600 },
    });
  });

  it("cancels without editing and triggers onClose", async () => {
    const { client, call } = makeMockClient();
    const onClose = vi.fn();
    const { container } = render(TimerEditForm, {
      props: {
        timer: makeOneShotTimer(),
        client: client as never,
        nowUnix,
        onClose,
      },
    });

    const cancel = Array.from(container.querySelectorAll("button")).find((el) =>
      el.textContent?.includes("Cancel"),
    ) as HTMLButtonElement;
    expect(cancel).toBeTruthy();
    await fireEvent.click(cancel);
    await tick();

    expect(call).toHaveBeenCalledTimes(0);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
