import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/svelte/svelte5";

vi.mock("@quantum/client", () => ({
  createClient: () => ({
    call: vi.fn().mockResolvedValue(undefined),
    subscribe: vi.fn(() => () => {}),
    close: vi.fn(),
  }),
  createTimerStore: () => ({
    subscribe: vi.fn(() => () => {}),
  }),
  __esModule: true,
}));

import App from "./App.svelte";

describe("Timers App", () => {
  it("renders the stage with zero timers without error", () => {
    const { container } = render(App);
    const stage = container.querySelector(".stage");
    expect(stage).not.toBeNull();
    expect(stage?.querySelectorAll(".slot").length).toBe(0);
  });
});
