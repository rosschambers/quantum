// Pure formatting helpers for the file-explorer rows. Kept in a plain module
// (not a `.svelte.ts`) so they carry no reactive state and are trivially
// unit-testable. Both functions are deterministic given their input, though
// `formatModified` renders in the host's LOCAL timezone by design.

const KILOBYTE = 1024;
const MEGABYTE = KILOBYTE * 1024;
const GIGABYTE = MEGABYTE * 1024;

/**
 * Render a byte count as a short human-readable string: bytes below one
 * kilobyte, then KB / MB / GB with a single decimal place. A whole number in
 * the chosen unit drops its fractional part (1024 becomes "1 KB", not
 * "1.0 KB"); a fractional value keeps one digit (1536 becomes "1.5 KB").
 */
export function formatSize(bytes: number): string {
    if (bytes >= GIGABYTE) {
        return `${trimOneDecimal(bytes / GIGABYTE)} GB`;
    }
    if (bytes >= MEGABYTE) {
        return `${trimOneDecimal(bytes / MEGABYTE)} MB`;
    }
    if (bytes >= KILOBYTE) {
        return `${trimOneDecimal(bytes / KILOBYTE)} KB`;
    }
    return `${bytes} B`;
}

/** Round to one decimal, then drop a trailing ".0" so whole values read cleanly. */
function trimOneDecimal(value: number): string {
    const rounded = Math.round(value * 10) / 10;
    return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
}

/**
 * Render a Unix epoch-seconds timestamp as `YYYY-MM-DD HH:MM` in LOCAL time,
 * zero-padded. Tests construct the expected string from `new Date` to stay
 * timezone-agnostic.
 */
export function formatModified(epochSeconds: number): string {
    const date = new Date(epochSeconds * 1000);
    const year = date.getFullYear();
    const month = pad(date.getMonth() + 1);
    const day = pad(date.getDate());
    const hours = pad(date.getHours());
    const minutes = pad(date.getMinutes());
    return `${year}-${month}-${day} ${hours}:${minutes}`;
}

/** Zero-pad a number to at least two digits. */
function pad(value: number): string {
    return String(value).padStart(2, '0');
}
