import type { IconRef } from './types';

/**
 * Resolve an icon reference to a URL the WebKit webview can load, or
 * `undefined` when there is nothing loadable.
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
export function resolveIcon(icon: string | IconRef | undefined): string | undefined {
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

/**
 * Whether an icon is an inline image thumbnail rather than an application glyph.
 *
 * A `data_uri` icon carries an embedded raster image (a clipboard image entry's
 * preview, or a mime-type thumbnail) that should cover-crop to a square tile,
 * unlike a `path` or `name` app icon which is a glyph shown contained. The
 * launcher uses this to pick the thumbnail styling for such rows.
 */
export function isThumbnailIcon(icon: string | IconRef | undefined): boolean {
  return typeof icon === 'object' && icon !== null && icon.kind === 'data_uri';
}
