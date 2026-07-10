// Pure path helpers for the file-explorer breadcrumb. Kept in a plain module
// (not a `.svelte.ts`) so it carries no reactive state and is trivially
// unit-testable. The one function here is deterministic given its input.

/** A single breadcrumb segment: the visible label and the absolute path a click jumps to. */
export interface BreadcrumbSegment {
    /** What the segment shows: "/" for the root, otherwise the directory name. */
    label: string;
    /** The absolute path up to and including this segment. */
    target: string;
}

/**
 * Split an absolute path into clickable breadcrumb segments. The first segment
 * is always the root ("/" targeting "/"); each subsequent segment is a
 * directory name targeting the accumulated absolute path up to and including
 * it. Examples:
 *   "/"                     -> [ / ]
 *   "/home"                 -> [ /, home->/home ]
 *   "/home/user/Documents"  -> [ /, home->/home, user->/home/user, Documents->/home/user/Documents ]
 */
export function breadcrumbSegments(path: string): BreadcrumbSegment[] {
    const segments: BreadcrumbSegment[] = [{ label: '/', target: '/' }];
    const parts = path.split('/').filter((part) => part.length > 0);
    let accumulated = '';
    for (const part of parts) {
        accumulated += `/${part}`;
        segments.push({ label: part, target: accumulated });
    }
    return segments;
}

/**
 * The absolute path and every ancestor above it, root first, leaf last. The
 * directory tree uses this to auto-expand the chain leading to the active
 * node so it is scrolled into view. Examples:
 *   "/"              -> ["/"]
 *   "/home"          -> ["/", "/home"]
 *   "/home/user/x"   -> ["/", "/home", "/home/user", "/home/user/x"]
 * A trailing slash is ignored ("/home/user/" is the same as "/home/user").
 */
export function ancestorPaths(path: string): string[] {
    const result = ['/'];
    const parts = path.split('/').filter((part) => part.length > 0);
    let accumulated = '';
    for (const part of parts) {
        accumulated += `/${part}`;
        result.push(accumulated);
    }
    return result;
}

/**
 * The display name for a directory node: "/" for the filesystem root, otherwise
 * the final path segment. A trailing slash is ignored.
 */
export function pathBaseName(path: string): string {
    const parts = path.split('/').filter((part) => part.length > 0);
    return parts.length === 0 ? '/' : parts[parts.length - 1];
}
