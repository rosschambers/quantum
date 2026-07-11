import type { IconRef } from '@quantum/client';

/**
 * Resolve an icon reference to a URL the WebKit webview can load, or
 * `undefined` when there is nothing loadable. This is a local copy of the
 * launcher's helper (packages are separate, so the tiny mapping is duplicated
 * rather than imported, matching how `Icon.svelte` is copied per plugin).
 *
 * - A plain string is returned as-is (already a URL).
 * - `path` references are absolute filesystem paths; they are served through
 *   the `quantum://icon/<percent-encoded-path>` scheme route, which validates
 *   the path against the allowed icon roots. The encoding here MUST match the
 *   decoder in `src/ui/host/src/scheme.rs` (`percent_decode`).
 * - `data_uri` references are returned verbatim.
 * - `name` references have no loadable URL (the backend resolves names to
 *   paths before they ever reach the frontend), so they yield `undefined`.
 */
export function resolveIcon(icon: string | IconRef | null | undefined): string | undefined {
    if (!icon) {
        return undefined;
    }
    if (typeof icon === 'string') {
        return icon;
    }
    if (icon.kind === 'path') {
        return `quantum://icon/${encodeURIComponent(icon.data)}`;
    }
    if (icon.kind === 'data_uri') {
        return icon.data;
    }
    return undefined;
}
