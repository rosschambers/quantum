// A thin, typed layer over a `@quantum/client` `Client` for the file-explorer
// view. Every method maps one-to-one onto a locked `files.*` daemon method,
// forwarding the exact snake_case parameter shape the daemon expects and
// returning the typed result. All IPC for the files view goes through here;
// view code never touches `window.__quantum_*` directly.

import {
    createClient,
    type Client,
    type FileEntry,
    type Places,
    type Pin,
    type ApplicationInfo,
    type FileOperation,
    type PreviewPayload,
    type FilesEvent,
} from '@quantum/client';

/** The typed file-explorer IPC surface. */
export interface FilesIpc {
    /** List the entries of a directory. */
    list(path: string): Promise<FileEntry[]>;
    /** Fetch the places sidebar snapshot: pins and mounted drives. */
    places(): Promise<Places>;
    /** Pin a location; resolves with the updated pin list. */
    pin(label: string, path: string): Promise<Pin[]>;
    /** Remove a pinned location; resolves with the updated pin list. */
    unpin(path: string): Promise<Pin[]>;
    /** Perform a file-system operation. */
    operation(op: FileOperation): Promise<void>;
    /** Open a path with its default application. */
    open(path: string): Promise<void>;
    /** Open a path with a specific application by desktop identifier. */
    openWith(path: string, desktopId: string): Promise<void>;
    /** List applications that can open files. */
    applications(): Promise<ApplicationInfo[]>;
    /** Open a terminal in the given directory. */
    openTerminal(directory: string): Promise<void>;
    /** Fetch a preview payload for a path. */
    preview(path: string): Promise<PreviewPayload>;
    /** Recursively search a root for a query, capped at limit results. */
    search(root: string, query: string, limit: number): Promise<FileEntry[]>;
    /** Start watching a directory for changes. */
    watch(path: string): Promise<void>;
    /** Stop watching a directory for changes. */
    unwatch(path: string): Promise<void>;
    /** Start recursive size computation for a directory. */
    sizes(path: string): Promise<void>;
    /** Cancel an in-flight recursive size computation. */
    cancelSizes(path: string): Promise<void>;
    /**
     * Subscribe to asynchronous file-explorer events on the `files.event`
     * channel. Returns the client's unsubscribe handle.
     */
    subscribeFilesEvents(callback: (event: FilesEvent) => void): () => void;
}

/**
 * Build a `FilesIpc` bound to a `Client`. The client is injectable so tests
 * can pass a fake; the default lazily creates the real one via `createClient`.
 */
export function createFilesIpc(client: Client = createClient()): FilesIpc {
    return {
        list(path: string): Promise<FileEntry[]> {
            return client.call('files.list', { path }) as Promise<FileEntry[]>;
        },
        places(): Promise<Places> {
            return client.call('files.places', {}) as Promise<Places>;
        },
        pin(label: string, path: string): Promise<Pin[]> {
            return client.call('files.pin', { label, path }) as Promise<Pin[]>;
        },
        unpin(path: string): Promise<Pin[]> {
            return client.call('files.unpin', { path }) as Promise<Pin[]>;
        },
        operation(op: FileOperation): Promise<void> {
            return client.call('files.operation', op) as Promise<void>;
        },
        open(path: string): Promise<void> {
            return client.call('files.open', { path }) as Promise<void>;
        },
        openWith(path: string, desktopId: string): Promise<void> {
            return client.call('files.open_with', {
                path,
                desktop_id: desktopId,
            }) as Promise<void>;
        },
        applications(): Promise<ApplicationInfo[]> {
            return client.call('files.applications', {}) as Promise<ApplicationInfo[]>;
        },
        openTerminal(directory: string): Promise<void> {
            return client.call('files.open_terminal', { directory }) as Promise<void>;
        },
        preview(path: string): Promise<PreviewPayload> {
            return client.call('files.preview', { path }) as Promise<PreviewPayload>;
        },
        search(root: string, query: string, limit: number): Promise<FileEntry[]> {
            return client.call('files.search', { root, query, limit }) as Promise<FileEntry[]>;
        },
        watch(path: string): Promise<void> {
            return client.call('files.watch', { path }) as Promise<void>;
        },
        unwatch(path: string): Promise<void> {
            return client.call('files.unwatch', { path }) as Promise<void>;
        },
        sizes(path: string): Promise<void> {
            return client.call('files.sizes', { path }) as Promise<void>;
        },
        cancelSizes(path: string): Promise<void> {
            return client.call('files.cancel_sizes', { path }) as Promise<void>;
        },
        subscribeFilesEvents(callback: (event: FilesEvent) => void): () => void {
            return client.subscribe(
                'files.event',
                callback as unknown as (payload: unknown) => void,
            );
        },
    };
}
