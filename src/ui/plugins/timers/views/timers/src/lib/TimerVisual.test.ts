import { describe, it, expect } from "vitest";
import { render } from "@testing-library/svelte/svelte5";
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
});
