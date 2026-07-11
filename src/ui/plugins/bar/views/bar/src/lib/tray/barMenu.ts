// Shared wiring for the bar's right-click quick-action menus.
//
// The bar surface is a full-height strip whose input region is gated to the
// visible bar strip; pointer events outside the strip pass through to windows
// beneath. A downward menu therefore needs the bar's input region expanded to
// cover it (onPlaced) for the lifetime of the menu, then reset back to the
// strip on close (onClose) via `view.set_input_region`.
//
// Each indicator builds its menu items dynamically from the provider state it
// already subscribes to, so `wireBarMenu` takes a `buildItems` closure that is
// evaluated fresh every time the menu opens.

import {
    closeContextMenu,
    openContextMenu,
    type Client,
    type MenuItem,
} from '@quantum/client';

/** The per-monitor global injected onto the bar webview by the host. */
function currentMonitor(): string | undefined {
    return (window as unknown as { __quantum_monitor?: string }).__quantum_monitor;
}

/**
 * Append the bar's `@<monitor>` suffix to a view base name when the bar knows
 * which monitor it is running on. Indicators that open per-monitor overlays
 * (notification center, power menu, and similar) use this so the overlay pins
 * to the same monitor as the bar that triggered it.
 */
export function monitorView(base: string): string {
    const monitor = currentMonitor();
    return monitor ? `${base}@${monitor}` : base;
}

/** The monitor-suffixed view name of the bar itself, used for input-region calls. */
export function barViewName(): string {
    return monitorView('plugin/bar/bar');
}

/**
 * Wire a quick-actions menu onto `node`. Returns a teardown that removes the
 * listener.
 *
 * `trigger` selects the DOM event that opens the menu: the default
 * `'contextmenu'` keeps every bar indicator on right-click, while `'click'`
 * opens the menu on a plain left-click (used by the kill-window button, whose
 * left-click must open the menu rather than force-kill a window).
 *
 * The menu drops down from below the button (true dropdown) via `anchorRect`.
 * `onPlaced` expands the bar's input region to cover the menu so it is
 * clickable; `onClose` resets the region to the strip. Both swallow errors via
 * `console.error` so a failed region call never breaks menu interaction.
 */
export function wireBarMenu(
    node: HTMLElement,
    client: Client,
    buildItems: () => MenuItem[] | Promise<MenuItem[]>,
    trigger: 'click' | 'contextmenu' = 'contextmenu',
): () => void {
    const expandInputRegion = (rect: {
        x: number;
        y: number;
        width: number;
        height: number;
    }): void => {
        client
            .call('view.set_input_region', {
                name: barViewName(),
                region: {
                    x: Math.round(rect.x),
                    y: Math.round(rect.y),
                    width: Math.round(rect.width),
                    height: Math.round(rect.height),
                },
            })
            .catch(console.error);
    };

    const resetInputRegion = (): void => {
        client
            .call('view.set_input_region', { name: barViewName(), region: null })
            .catch(console.error);
    };

    // Open the menu from a resolved item list. The anchor rectangle is read
    // here (after any async resolve) so the menu drops from the button's
    // current position rather than where it was when right-clicked.
    const open = (event: MouseEvent, built: MenuItem[]): void => {
        const items = normalizeSeparators(built);
        // An indicator whose provider is unavailable builds no items; opening
        // an empty floating box and expanding the input region for it is both
        // useless and a momentary pointer-capture lockout, so bail early.
        if (items.length === 0) return;
        openContextMenu(event, items, {
            anchorRect: node.getBoundingClientRect(),
            onPlaced: expandInputRegion,
            onClose: resetInputRegion,
        });
    };

    const listener = (event: MouseEvent): void => {
        // Suppress the browser context menu synchronously, before any await,
        // so a builder that fetches its items asynchronously (for example the
        // kill menu querying the window list) cannot let the default menu slip
        // through while the promise resolves.
        event.preventDefault();
        const built = buildItems();
        // A synchronous builder (every indicator other than the kill menu)
        // opens the menu in the same tick, preserving its existing timing. An
        // async builder defers the open until the promise resolves.
        if (built instanceof Promise) {
            built.then((items) => open(event, items)).catch(console.error);
        } else {
            open(event, built);
        }
    };

    node.addEventListener(trigger, listener);
    return () => {
        node.removeEventListener(trigger, listener);
        // Unmounting an indicator while its menu is open must dismiss the menu
        // and reset the bar's input region via the runtime's onClose hook.
        closeContextMenu();
    };
}

/**
 * Drop leading, trailing, and consecutive separators. Indicators build their
 * items by conditionally pushing entries (gated on capability/state), which can
 * leave a separator with nothing on one side; this keeps every menu's dividers
 * meaningful regardless of which entries were included.
 */
function normalizeSeparators(items: MenuItem[]): MenuItem[] {
    return items.filter(
        (item, index, list) =>
            !item.separator ||
            (index > 0 && index < list.length - 1 && !list[index - 1].separator),
    );
}
