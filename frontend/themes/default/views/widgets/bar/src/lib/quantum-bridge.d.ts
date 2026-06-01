/*
 * TypeScript declarations for window globals injected by the Rust
 * side at WebView load time. Keeping these in a shared `.d.ts` lets
 * both the Svelte components and the vitest specs read them
 * without `as any` casts.
 *
 * Currently only `__quantum_monitor` lives here. The other
 * `window.__quantum_*` symbols are owned by `@quantum/client`'s
 * bridge module and shouldn't be referenced directly from view
 * code (per AGENTS.md).
 */
declare global {
    interface Window {
        /**
         * Wayland connector name of the monitor this WebView is
         * pinned to (e.g. "DP-1"). Set by
         * `crates/ui/src/windows/widget.rs` BEFORE the Svelte app
         * mounts, when the widget is constructed with a non-None
         * `monitor` argument. Absent for widgets that aren't pinned
         * to a specific monitor.
         */
        __quantum_monitor?: string;
    }
}

export {};
