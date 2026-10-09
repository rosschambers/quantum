// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { onShown } from './shown';

afterEach(() => {
  vi.restoreAllMocks();
});

function fireShown(): void {
  window.dispatchEvent(new CustomEvent('quantum:shown'));
}

describe('onShown', () => {
  it('invokes the callback each time the quantum:shown event fires', () => {
    const callback = vi.fn();
    onShown(callback);

    fireShown();
    fireShown();

    expect(callback).toHaveBeenCalledTimes(2);
  });

  it('does not invoke the callback before any event fires', () => {
    const callback = vi.fn();
    onShown(callback);

    expect(callback).not.toHaveBeenCalled();
  });

  it('stops invoking the callback after the returned unsubscribe is called', () => {
    const callback = vi.fn();
    const off = onShown(callback);

    fireShown();
    off();
    fireShown();

    expect(callback).toHaveBeenCalledTimes(1);
  });

  it('returns a no-op unsubscribe when window is undefined', () => {
    const original = globalThis.window;
    // Simulate a non-browser environment.
    // @ts-expect-error deliberately removing window for the test
    delete globalThis.window;
    try {
      const callback = vi.fn();
      const off = onShown(callback);
      expect(() => off()).not.toThrow();
      expect(callback).not.toHaveBeenCalled();
    } finally {
      globalThis.window = original;
    }
  });
});
