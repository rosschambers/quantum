import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/svelte/svelte5";
import type {
  Timer,
  VisualConfig,
  NotifyConfig,
} from "@quantum/client";
import TimerVisual from "./TimerVisual.svelte";

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

function makeTimer(overrides: Partial<VisualConfig>): Timer {
  return {
    id: "timer-under-test",
    label: "Tea",
    kind: { type: "one_shot", end_unix: nowUnix + 300 },
    visual: { ...defaultVisual(), ...overrides },
    notify: defaultNotify(),
    status: "active",
    scatter_pos: null,
  };
}

describe("TimerVisual", () => {
  it("renders pie as an svg sector with a fill border", () => {
    const pieTimer = makeTimer({ style: "pie", fill_border: true });
    const { container } = render(TimerVisual, {
      props: { timer: pieTimer, nowUnix, indexInList: 0 },
    });
    const path = container.querySelector("svg path");
    expect(path).not.toBeNull();
    expect(path?.getAttribute("stroke")).not.toBe("none");
  });

  it("renders spiral as an svg path with a non-empty d attribute", () => {
    const spiralTimer = makeTimer({ style: "spiral" });
    const { container } = render(TimerVisual, {
      props: { timer: spiralTimer, nowUnix, indexInList: 0 },
    });
    const path = container.querySelector("svg path");
    expect(path).not.toBeNull();
    const d = path?.getAttribute("d");
    expect(typeof d).toBe("string");
    expect(d).not.toBe("");
  });

  it("renders pulse as a filled svg circle without error", () => {
    const pulseTimer = makeTimer({ style: "pulse" });
    const { container } = render(TimerVisual, {
      props: { timer: pulseTimer, nowUnix, indexInList: 0 },
    });
    const circle = container.querySelector("svg circle");
    expect(circle).not.toBeNull();
  });

  it("invokes onDismiss when the dismiss control is clicked", async () => {
    const onDismiss = vi.fn();
    const { container } = render(TimerVisual, {
      props: { timer: makeTimer({}), nowUnix, indexInList: 0, onDismiss },
    });
    const button = container.querySelector(
      '[aria-label="Dismiss timer"]',
    ) as HTMLButtonElement;
    expect(button).not.toBeNull();
    await fireEvent.click(button);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("invokes onEdit when the edit control is clicked", async () => {
    const onEdit = vi.fn();
    const { container } = render(TimerVisual, {
      props: { timer: makeTimer({}), nowUnix, indexInList: 0, onEdit },
    });
    const button = container.querySelector(
      '[aria-label="Edit timer"]',
    ) as HTMLButtonElement;
    expect(button).not.toBeNull();
    await fireEvent.click(button);
    expect(onEdit).toHaveBeenCalledTimes(1);
  });
});
