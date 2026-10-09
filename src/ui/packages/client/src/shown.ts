/**
 * Re-show notification for warm (reused) views.
 *
 * A view whose `view.toml` sets `destroy_on_dismiss = false` is kept resident
 * and only hidden on dismiss, then reshown on the next open — its component is
 * never remounted, so any open-time work in a Svelte `$effect` (clearing a
 * pending confirmation, refreshing a snapshot, re-opening a provider session)
 * runs only once, at first mount. The host fires a `quantum:shown` window event
 * every time it reshows the window (see `dispatch_view_shown` in the UI host);
 * `onShown` wraps that event so a warm view can re-run its open logic.
 *
 * Cold (`destroy_on_dismiss = true`) views are reconstructed on every open, so
 * their mount logic already runs each time and they need not call this — the
 * event still fires but no listener is attached.
 */
export function onShown(callback: () => void): () => void {
  if (typeof window === 'undefined') {
    return () => {};
  }
  const handler = (): void => callback();
  window.addEventListener('quantum:shown', handler);
  return () => window.removeEventListener('quantum:shown', handler);
}
