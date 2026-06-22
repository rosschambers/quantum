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

import { openContextMenu, type Client, type MenuItem } from '@quantum/client';

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
 * Wire a right-click quick-actions menu onto `node`. Returns a teardown that
 * removes the listener.
 *
 * The menu drops down from below the button (true dropdown) via `anchorRect`.
 * `onPlaced` expands the bar's input region to cover the menu so it is
 * clickable; `onClose` resets the region to the strip. Both swallow errors via
 * `console.error` so a failed region call never breaks menu interaction.
 */
export function wireBarMenu(
    node: HTMLElement,
    client: Client,
    buildItems: () => MenuItem[],
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

    const listener = (event: MouseEvent): void => {
        openContextMenu(event, buildItems(), {
            anchorRect: node.getBoundingClientRect(),
            onPlaced: expandInputRegion,
            onClose: resetInputRegion,
        });
    };

    node.addEventListener('contextmenu', listener);
    return () => node.removeEventListener('contextmenu', listener);
}
