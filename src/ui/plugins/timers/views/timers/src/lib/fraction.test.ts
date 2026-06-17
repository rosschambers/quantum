import { describe, it, expect } from "vitest";
import {
  remainingFraction,
  displayFraction,
  rampColor,
  formatTime,
} from "./fraction";

describe("remainingFraction", () => {
  it("returns the open fraction of time still remaining", () => {
    // 30 of 60 seconds left -> half remaining.
    expect(remainingFraction(100, 130, 60)).toBeCloseTo(0.5, 5);
  });

  it("clamps to 1 when more time remains than the recorded total", () => {
    // firesAt is 120s out but total is only 60s -> clamp high.
    expect(remainingFraction(0, 120, 60)).toBe(1);
  });

  it("clamps to 0 once the timer has already passed", () => {
    // firesAt is in the past relative to now.
    expect(remainingFraction(200, 130, 60)).toBe(0);
  });

  it("returns 0 when totalSecs is zero", () => {
    expect(remainingFraction(0, 30, 0)).toBe(0);
  });

  it("returns 0 when totalSecs is negative", () => {
    expect(remainingFraction(0, 30, -10)).toBe(0);
  });
});

describe("displayFraction", () => {
  it("passes the fraction through unchanged when not filling", () => {
    expect(displayFraction(0.25, false)).toBeCloseTo(0.25, 5);
  });

  it("inverts the fraction when filling", () => {
    expect(displayFraction(0.25, true)).toBeCloseTo(0.75, 5);
  });

  it("inverts the endpoints", () => {
    expect(displayFraction(0, true)).toBe(1);
    expect(displayFraction(1, true)).toBe(0);
  });
});

describe("rampColor", () => {
  it("returns the base hue when above the threshold", () => {
    // 50% remaining, threshold 20% -> no ramp, base hue 210.
    expect(rampColor(0.5, 210, 20)).toBe("hsl(210 70% 60%)");
  });

  it("reddens fully (hue 0) at zero remaining when in ramp", () => {
    expect(rampColor(0, 210, 20)).toBe("hsl(0 70% 60%)");
  });

  it("partially reddens between the threshold and zero", () => {
    // 10% remaining, threshold 20% -> t = 1 - (0.1 / 0.2) = 0.5,
    // hue = lerp(210, 0, 0.5) = 105.
    expect(rampColor(0.1, 210, 20)).toBe("hsl(105 70% 60%)");
  });

  it("stays at the base hue exactly at the threshold boundary", () => {
    // frac == threshold/100 -> t = 0 -> hue unchanged.
    expect(rampColor(0.2, 210, 20)).toBe("hsl(210 70% 60%)");
  });
});

describe("formatTime", () => {
  it("formats clock as m:ss", () => {
    expect(formatTime(125, "clock", 0.5)).toBe("2:05");
  });

  it("formats clock with hours when over an hour", () => {
    expect(formatTime(3725, "clock", 0.5)).toBe("1:02:05");
  });

  it("formats compact in minutes above one minute", () => {
    expect(formatTime(125, "compact", 0.5)).toBe("2m");
  });

  it("formats compact in seconds below one minute", () => {
    expect(formatTime(45, "compact", 0.5)).toBe("45s");
  });

  it("formats percent from the true fraction, not the seconds", () => {
    expect(formatTime(125, "percent", 0.37)).toBe("37%");
  });

  it("never shows negative time", () => {
    expect(formatTime(-5, "clock", 0)).toBe("0:00");
  });
});
