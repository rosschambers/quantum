<script lang="ts">
    /**
     * The file-explorer application shell. It assembles the toolbar, sidebar,
     * one or two directory panes, the automatic preview pane, the status bar,
     * the toast stack, and the properties / name-prompt modals, and owns every
     * cross-component concern: which pane is active, per-pane data loading,
     * the single `files.event` subscription, window-level keyboard handling,
     * context menus, file operations, and drag-and-drop moves.
     *
     * State (`PaneState`) lives in each pane; the App performs the IPC. Two
     * per-pane loader effects list the directory, watch it for changes, and
     * request recursive sizes on navigation; a single event handler reloads a
     * pane on `changed`, streams folder sizes in on `size`, and toasts operation
     * failures. Keyboard, selection, and live filtering all act on the active
     * pane: bare printable typing narrows the active pane's list through its
     * filter, Backspace edits that filter (or navigates up when it is empty),
     * and Escape clears it (or closes menus/modals when it is empty).
     */
    import type {
        ApplicationInfo,
        DriveInfo,
        FileEntry,
        FileOperation,
        FilesEvent,
        Pin,
    } from '@quantum/client';
    import { openContextMenu, closeContextMenu } from '@quantum/client';
    import { createFilesIpc, type FilesIpc } from './lib/ipc';
    import { PaneState, type SortColumn } from './lib/paneState.svelte';
    import Toolbar from './lib/Toolbar.svelte';
    import Sidebar from './lib/Sidebar.svelte';
    import FileList from './lib/FileList.svelte';
    import PreviewPane from './lib/PreviewPane.svelte';
    import StatusBar from './lib/StatusBar.svelte';
    import Toasts from './lib/Toasts.svelte';
    import PropertiesModal from './lib/PropertiesModal.svelte';
    import PromptModal from './lib/PromptModal.svelte';
    import ConfirmModal from './lib/ConfirmModal.svelte';
    import { resolveShortcut, type ShortcutAction } from './lib/keymap';
    import { buildEntryMenu, buildBackgroundMenu, type PinTarget } from './lib/menus';
    import { runOperation, type OperationResult } from './lib/operations';
    import {
        setClipboard,
        getClipboard,
        clearClipboard,
        type ClipboardOperation,
    } from './lib/clipboard.svelte';
    import { pushToast } from './lib/toasts.svelte';
    import { isPreviewable } from './lib/preview';
    import { formatSize } from './lib/format';

    /** A properties target: always a name and path, optionally full entry metadata. */
    type PropertiesTarget = Partial<FileEntry> & { name: string; path: string };
    /** An open name-prompt request (rename / new folder / new file). */
    interface PromptRequest {
        title: string;
        initial: string;
        onSubmit: (value: string) => void;
    }
    /** An open confirmation request (destructive action, for example permanent delete). */
    interface ConfirmRequest {
        title: string;
        message: string;
        confirmLabel: string;
        onConfirm: () => void;
    }

    interface Props {
        /** The typed IPC surface; injectable so tests pass a fake. */
        ipc?: FilesIpc;
    }

    const { ipc = createFilesIpc() }: Props = $props();

    // Both panes start at the filesystem root and are navigated to Home once
    // `places()` resolves (the frontend cannot read $HOME). The array itself is
    // stable — panes are mutated in place, never reassigned — so the rendered
    // pane subtrees are never torn down and recreated.
    const panes: PaneState[] = [new PaneState('/'), new PaneState('/')];
    let activePaneIndex = $state(0);
    let dualPane = $state(true);
    let pins = $state<Pin[]>([]);
    let drives = $state<DriveInfo[]>([]);

    // Per-pane keyboard cursor and selection anchor, indexed by pane.
    let cursors = $state<number[]>([0, 0]);
    let anchors = $state<number[]>([0, 0]);

    let breadcrumbEditing = $state(false);
    let propertiesTarget = $state<PropertiesTarget | null>(null);
    let promptRequest = $state<PromptRequest | null>(null);
    let confirmRequest = $state<ConfirmRequest | null>(null);
    let cachedApplications = $state<ApplicationInfo[]>([]);

    // Plain (non-reactive) scratch: the event that opened the current menu (so a
    // secondary "Open with" menu can drop from the same spot) and the pending
    // drive-refresh debounce timer.
    let lastMenuEvent: MouseEvent | null = null;
    let driveRefreshTimer: ReturnType<typeof setTimeout> | undefined;

    /** How long to collapse a burst of file events into a single drive refetch. */
    const DRIVE_REFRESH_DEBOUNCE_MS = 1000;

    // A recursive delete fires many `changed` events; refetching drives per
    // event would hammer the daemon. Instead, clear and reset a single timer so
    // a burst collapses into one `refreshPlaces()` once it settles.
    function scheduleDriveRefresh(): void {
        clearTimeout(driveRefreshTimer);
        driveRefreshTimer = setTimeout(() => {
            void refreshPlaces();
        }, DRIVE_REFRESH_DEBOUNCE_MS);
    }

    const active = $derived(panes[activePaneIndex]);
    const otherIndex = $derived(activePaneIndex === 0 ? 1 : 0);
    const activeVisible = $derived(active.visibleEntries());

    // The preview column is allocated only when the active pane has exactly one
    // selected entry and that entry is previewable.
    const previewEntry = $derived.by((): FileEntry | null => {
        const selected = [...active.selection];
        if (selected.length !== 1) {
            return null;
        }
        const entry = active.entries.find((candidate) => candidate.path === selected[0]);
        return entry !== undefined && isPreviewable(entry) ? entry : null;
    });

    // Status-bar figures for the active pane.
    const selectionBytes = $derived.by((): number => {
        let total = 0;
        for (const entry of active.entries) {
            if (active.selection.has(entry.path)) {
                total += entry.recursive_size ?? entry.size;
            }
        }
        return total;
    });
    const activeDrive = $derived.by((): DriveInfo | null => {
        let best: DriveInfo | null = null;
        for (const drive of drives) {
            const matches = active.path === drive.mount_point || active.path.startsWith(`${drive.mount_point}/`);
            if (matches && (best === null || drive.mount_point.length > best.mount_point.length)) {
                best = drive;
            }
        }
        return best;
    });

    /** The largest entry size in a pane, driving the row usage bars. */
    function maxEntrySize(entries: FileEntry[]): number {
        let max = 0;
        for (const entry of entries) {
            const size = entry.recursive_size ?? entry.size;
            if (size > max) {
                max = size;
            }
        }
        return max;
    }

    function clamp(value: number, low: number, high: number): number {
        return Math.max(low, Math.min(value, high));
    }

    /** Toast an operation result: neutral on success, error on failure. */
    function toastResult(result: OperationResult): void {
        pushToast(result.message, result.ok ? 'info' : 'error');
    }

    /** Run a file operation and toast its outcome. The `changed` event reloads. */
    function runOp(op: FileOperation): void {
        void runOperation(ipc, op, { onDone: toastResult });
    }

    /** Load a pane's directory (or its search results when deep search is active). */
    async function loadPane(pane: PaneState): Promise<void> {
        pane.loading = true;
        try {
            if (pane.deepSearch && pane.filter.trim() !== '') {
                pane.entries = await ipc.search(pane.path, pane.filter, 500);
            } else {
                pane.entries = await ipc.list(pane.path);
            }
        } catch {
            pushToast(`Failed to load ${pane.path}`, 'error');
        } finally {
            pane.loading = false;
        }
    }

    /** Re-fetch pins and drives (after a pin/unpin mutation or at startup). */
    async function refreshPlaces(): Promise<void> {
        try {
            const places = await ipc.places();
            pins = places.pins;
            drives = places.drives;
        } catch {
            // Leave the last known places in place on a transient failure.
        }
    }

    // Startup: load places, then seed both panes at Home (the first pin) so the
    // history starts clean rather than carrying the placeholder root.
    let initialized = false;
    $effect(() => {
        if (initialized) {
            return;
        }
        initialized = true;
        void ipc
            .places()
            .then((places) => {
                pins = places.pins;
                drives = places.drives;
                const home = places.pins[0]?.path ?? '/';
                if (home !== '/') {
                    panes[0].navigate(home);
                    panes[1].navigate(home);
                }
            })
            .catch(() => {
                // Fall back to the root-seeded panes already in place.
            });
        // Load the persisted show-hidden preference independently of the places
        // load; on failure keep the default (both panes showing hidden files).
        void ipc
            .getPreferences()
            .then((preferences) => {
                panes[0].showHidden = preferences.show_hidden;
                panes[1].showHidden = preferences.show_hidden;
            })
            .catch(() => {
                // Keep the default showHidden of both panes.
            });
    });

    // The single files.event subscription: reload changed panes, stream folder
    // sizes into whichever pane holds the entry, and toast operation failures.
    $effect(() => {
        const unsubscribe = ipc.subscribeFilesEvents(handleFilesEvent);
        return unsubscribe;
    });

    function handleFilesEvent(event: FilesEvent): void {
        if (event.event === 'changed') {
            for (const pane of panes) {
                if (pane.path === event.path) {
                    void loadPane(pane);
                }
            }
            // A change to any watched directory can alter free space (a delete,
            // a move); refresh the sidebar drives after the burst settles.
            scheduleDriveRefresh();
        } else if (event.event === 'size') {
            for (const pane of panes) {
                const entry = pane.entries.find((candidate) => candidate.path === event.path);
                if (entry !== undefined) {
                    entry.recursive_size = event.bytes;
                }
            }
        } else if (event.event === 'operation_complete') {
            // Intentionally silent for toasts: successful operations are already
            // toasted by `runOperation`'s onDone, so echoing the event would
            // double the toast. It still refreshes drive free space.
            scheduleDriveRefresh();
        } else if (event.event === 'operation_failed') {
            pushToast(event.message, 'error');
        }
    }

    // Per-pane loader: on navigation (path change) list the directory, watch it,
    // and request recursive sizes; on cleanup unwatch and cancel sizes. Declared
    // via a helper so both panes share one definition.
    function setupPaneLoader(index: number): void {
        $effect(() => {
            const pane = panes[index];
            const path = pane.path;
            void ipc.watch(path).catch(() => {});
            void ipc.sizes(path).catch(() => {});
            void loadPane(pane);
            cursors[index] = 0;
            anchors[index] = 0;
            return () => {
                void ipc.unwatch(path).catch(() => {});
                void ipc.cancelSizes(path).catch(() => {});
            };
        });
    }
    setupPaneLoader(0);
    setupPaneLoader(1);

    // Window-level keyboard handling. Ignored while a text input is focused,
    // except for Escape which clears an active filter or closes an open menu or
    // modal.
    $effect(() => {
        function onKeyDown(event: KeyboardEvent): void {
            const target = event.target;
            const inInput =
                target instanceof HTMLElement &&
                (target.tagName === 'INPUT' || target.isContentEditable);

            if (event.key === 'Escape') {
                // A non-empty filter is cleared first and swallows the Escape;
                // only an empty filter falls through to closing menus/modals.
                if (active.filter !== '') {
                    handleFilterInput('');
                    return;
                }
                const hadOpen =
                    propertiesTarget !== null ||
                    promptRequest !== null ||
                    confirmRequest !== null;
                closeContextMenu();
                if (hadOpen) {
                    propertiesTarget = null;
                    promptRequest = null;
                    confirmRequest = null;
                } else if (!inInput) {
                    // Only a bare Escape over the file list clears the selection;
                    // Escape while typing in the filter or location bar must not.
                    active.clearSelection();
                }
                return;
            }

            if (inInput) {
                return;
            }

            const action = resolveShortcut(event);
            if (action !== null && action.kind !== 'clear-selection') {
                event.preventDefault();
                dispatchShortcut(action);
                return;
            }

            const pane = active;
            const index = activePaneIndex;

            if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                const visible = pane.visibleEntries();
                if (visible.length === 0) {
                    return;
                }
                event.preventDefault();
                const direction = event.key === 'ArrowDown' ? 1 : -1;
                const next = clamp(cursors[index] + direction, 0, visible.length - 1);
                if (event.shiftKey) {
                    pane.selectRange(anchors[index], next);
                } else {
                    pane.selectOnly(visible[next].path);
                    anchors[index] = next;
                }
                cursors[index] = next;
            } else if (event.key === 'Enter') {
                const entry = pane.visibleEntries()[cursors[index]];
                if (entry !== undefined) {
                    openEntry(index, entry);
                }
            } else if (event.key === 'Backspace') {
                // While filtering, Backspace edits the filter; with an empty
                // filter it keeps its "go up" behavior.
                if (pane.filter !== '') {
                    handleFilterInput(pane.filter.slice(0, -1));
                } else {
                    pane.up();
                }
            } else if (event.altKey && event.key === 'ArrowLeft') {
                pane.back();
            } else if (event.altKey && event.key === 'ArrowRight') {
                pane.forward();
            } else if (event.altKey && event.key === 'ArrowUp') {
                pane.up();
            } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'l') {
                event.preventDefault();
                breadcrumbEditing = true;
            } else if (event.key === 'Tab' && dualPane) {
                event.preventDefault();
                activePaneIndex = otherIndex;
            } else if (isPrintable(event)) {
                // Bare printable typing filters the active pane's list live,
                // appending to the same filter the toolbar filter box uses.
                handleFilterInput(pane.filter + event.key);
            }
        }
        window.addEventListener('keydown', onKeyDown);
        return () => window.removeEventListener('keydown', onKeyDown);
    });

    /** Whether a keydown is a bare printable character (no modifier). */
    function isPrintable(event: KeyboardEvent): boolean {
        return (
            event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey
        );
    }

    /** Accumulate a type-ahead buffer and select the first name-prefix match. */
    function typeAhead(character: string): void {
        clearTimeout(typeaheadTimer);
        typeaheadBuffer += character;
        typeaheadTimer = setTimeout(() => {
            typeaheadBuffer = '';
        }, 900);

        const pane = active;
        const index = activePaneIndex;
        const visible = pane.visibleEntries();
        const prefix = typeaheadBuffer.toLowerCase();
        const matchIndex = visible.findIndex((entry) => entry.name.toLowerCase().startsWith(prefix));
        if (matchIndex >= 0) {
            pane.selectOnly(visible[matchIndex].path);
            anchors[index] = matchIndex;
            cursors[index] = matchIndex;
        }
    }

    // ── Keyboard shortcuts ──────────────────────────────────────────────────

    /** The paths a management shortcut targets: the selection, else the cursor entry. */
    function targetPaths(): string[] {
        const selected = [...active.selection];
        if (selected.length > 0) {
            return selected;
        }
        const entry = active.visibleEntries()[cursors[activePaneIndex]];
        return entry !== undefined ? [entry.path] : [];
    }

    /** Clamp a pane's cursor and anchor into its current visible range. */
    function clampCursor(index: number): void {
        const length = panes[index].visibleEntries().length;
        const high = Math.max(0, length - 1);
        cursors[index] = clamp(cursors[index], 0, high);
        anchors[index] = clamp(anchors[index], 0, high);
    }

    function dispatchShortcut(action: ShortcutAction): void {
        const index = activePaneIndex;
        const paths = targetPaths();
        switch (action.kind) {
            case 'select-all':
                active.selectAll();
                break;
            case 'clipboard':
                if (paths.length > 0) {
                    setClipboard(action.operation, paths);
                }
                break;
            case 'paste':
                paste(active.path);
                break;
            case 'trash':
                if (paths.length > 0) {
                    runOp({ kind: 'trash', paths });
                }
                break;
            case 'delete-permanent':
                if (paths.length > 0) {
                    confirmRequest = {
                        title: 'Delete permanently',
                        message: `Permanently delete ${paths.length} item${paths.length === 1 ? '' : 's'}? This cannot be undone.`,
                        confirmLabel: 'Delete',
                        onConfirm: () => {
                            runOp({ kind: 'delete', paths });
                        },
                    };
                }
                break;
            case 'rename': {
                const entry = active.visibleEntries()[cursors[index]];
                if (entry !== undefined) {
                    openPrompt('Rename', entry.name, (name) =>
                        runOp({ kind: 'rename', path: entry.path, new_name: name }),
                    );
                }
                break;
            }
            case 'duplicate':
                for (const path of paths) {
                    runOp({ kind: 'duplicate', path });
                }
                break;
            case 'new-folder':
                openPrompt('New folder', 'New Folder', (name) =>
                    runOp({ kind: 'new_folder', parent: active.path, name }),
                );
                break;
            case 'refresh':
                void loadPane(active);
                break;
            case 'cursor': {
                const visible = active.visibleEntries();
                if (visible.length === 0) {
                    break;
                }
                const next = action.to === 'first' ? 0 : visible.length - 1;
                active.selectOnly(visible[next].path);
                cursors[index] = next;
                anchors[index] = next;
                break;
            }
            case 'toggle-hidden':
                toggleHidden();
                break;
            case 'clear-selection':
                active.clearSelection();
                break;
            default: {
                // Compile-time exhaustiveness: a new ShortcutAction variant that
                // is not handled above becomes a type error here rather than a
                // silent no-op.
                const unhandled: never = action;
                void unhandled;
            }
        }
    }

    /** Flip the show-hidden preference on both panes and re-clamp cursors. */
    function toggleHidden(): void {
        const next = !panes[0].showHidden;
        panes[0].showHidden = next;
        panes[1].showHidden = next;
        clampCursor(0);
        clampCursor(1);
        void ipc.setPreferences({ show_hidden: next }).catch(() => {
            pushToast('Failed to save hidden-files preference', 'error');
        });
    }

    // ── Selection and opening ───────────────────────────────────────────────

    function handleSelect(index: number, path: string, event: MouseEvent): void {
        activePaneIndex = index;
        const pane = panes[index];
        const visible = pane.visibleEntries();
        const clicked = visible.findIndex((entry) => entry.path === path);
        if (event.shiftKey) {
            pane.clearSelection();
            pane.selectRange(anchors[index], clicked);
            cursors[index] = clicked;
        } else if (event.ctrlKey || event.metaKey) {
            pane.toggleSelect(path);
            anchors[index] = clicked;
            cursors[index] = clicked;
        } else {
            pane.selectOnly(path);
            anchors[index] = clicked;
            cursors[index] = clicked;
        }
    }

    /** Open an entry: directories navigate, symlinks resolve, files open. */
    function openEntry(index: number, entry: FileEntry): void {
        activePaneIndex = index;
        const pane = panes[index];
        if (entry.kind === 'directory') {
            pane.navigate(entry.path);
            return;
        }
        if (entry.kind === 'symlink') {
            if (entry.symlink_target !== null) {
                const targetPath = entry.symlink_target;
                void ipc
                    .list(targetPath)
                    .then(() => pane.navigate(targetPath))
                    .catch(() => ipc.open(entry.path));
                return;
            }
            void ipc.open(entry.path);
            return;
        }
        void ipc.open(entry.path);
    }

    // ── Navigation helpers ──────────────────────────────────────────────────

    /** Validate a typed path by listing it before navigating the active pane. */
    async function navigateValidated(path: string): Promise<void> {
        try {
            await ipc.list(path);
            active.navigate(path);
        } catch {
            pushToast(`No such path: ${path}`, 'error');
        }
    }

    // ── Filter and deep search ──────────────────────────────────────────────

    function handleFilterInput(value: string): void {
        const pane = active;
        pane.filter = value;
        if (pane.deepSearch) {
            void loadPane(pane);
        }
    }

    function toggleDeep(): void {
        const pane = active;
        pane.deepSearch = !pane.deepSearch;
        void loadPane(pane);
    }

    // ── Sidebar and drag-and-drop ───────────────────────────────────────────

    function navigateActive(path: string): void {
        active.navigate(path);
    }

    function navigateOther(path: string): void {
        panes[otherIndex].navigate(path);
    }

    function handleMove(sources: string[], destination: string): void {
        runOp({ kind: 'move', sources, destination });
    }

    async function pinTarget(target: PinTarget): Promise<void> {
        try {
            await ipc.pin(target.name, target.path);
            await refreshPlaces();
        } catch {
            pushToast(`Failed to pin ${target.name}`, 'error');
        }
    }

    async function unpin(path: string): Promise<void> {
        try {
            await ipc.unpin(path);
            await refreshPlaces();
        } catch {
            pushToast('Failed to unpin', 'error');
        }
    }

    /** Paste the in-view clipboard into a directory as a copy or move. */
    function paste(directory: string): void {
        const clipboard = getClipboard();
        if (clipboard === null) {
            return;
        }
        const op: FileOperation =
            clipboard.operation === 'cut'
                ? { kind: 'move', sources: clipboard.paths, destination: directory }
                : { kind: 'copy', sources: clipboard.paths, destination: directory };
        runOp(op);
        if (clipboard.operation === 'cut') {
            clearClipboard();
        }
    }

    // ── Name prompt (rename / new folder / new file) ────────────────────────

    function openPrompt(title: string, initial: string, onSubmit: (value: string) => void): void {
        promptRequest = { title, initial, onSubmit };
    }

    function submitPrompt(value: string): void {
        const request = promptRequest;
        promptRequest = null;
        request?.onSubmit(value);
    }

    // ── Context menus ───────────────────────────────────────────────────────

    async function openApplicationPicker(entry: FileEntry): Promise<void> {
        const applications = await ipc.applications();
        cachedApplications = applications;
        if (lastMenuEvent === null) {
            return;
        }
        openContextMenu(
            lastMenuEvent,
            applications.map((application) => ({
                label: application.name,
                onSelect: () => {
                    void ipc.openWith(entry.path, application.id);
                },
            })),
        );
    }

    function handleEntryContextMenu(index: number, entry: FileEntry, event: MouseEvent): void {
        activePaneIndex = index;
        lastMenuEvent = event;
        const pane = panes[index];
        openContextMenu(
            event,
            buildEntryMenu({
                entry,
                selectionPaths: [...pane.selection],
                path: pane.path,
                isPinned: (path) => pins.some((pin) => pin.path === path),
                applications: cachedApplications,
                onOpen: (opened) => openEntry(index, opened),
                onOpenWithPicker: (target) => void openApplicationPicker(target),
                onOpenTerminal: (directory) => void ipc.openTerminal(directory),
                onClipboard: (operation: ClipboardOperation, paths) => setClipboard(operation, paths),
                onPaste: paste,
                onOperation: runOp,
                onRename: (target) =>
                    openPrompt('Rename', target.name, (name) =>
                        runOp({ kind: 'rename', path: target.path, new_name: name }),
                    ),
                onPin: (target) => void pinTarget(target),
                onUnpin: (path) => void unpin(path),
                onCopyPath: (path) => {
                    void navigator.clipboard?.writeText(path);
                },
                onProperties: (target) => {
                    propertiesTarget = target;
                },
            }),
        );
    }

    function handleBackgroundContextMenu(index: number, event: MouseEvent): void {
        if (event.defaultPrevented) {
            return;
        }
        activePaneIndex = index;
        lastMenuEvent = event;
        const pane = panes[index];
        openContextMenu(
            event,
            buildBackgroundMenu({
                path: pane.path,
                onNewFolder: (directory) =>
                    openPrompt('New folder', 'New Folder', (name) =>
                        runOp({ kind: 'new_folder', parent: directory, name }),
                    ),
                onNewFile: (directory) =>
                    openPrompt('New file', 'New File', (name) =>
                        runOp({ kind: 'new_file', parent: directory, name }),
                    ),
                onPaste: paste,
                onOpenTerminal: (directory) => void ipc.openTerminal(directory),
                onPin: (target) => void pinTarget(target),
                onProperties: (target) => {
                    propertiesTarget = target;
                },
            }),
        );
    }

    function sortColumn(pane: PaneState, column: SortColumn): void {
        pane.toggleSort(column);
    }
</script>

<div id="win" class:dual={dualPane}>
    <Toolbar
        path={active.path}
        canGoBack={active.canGoBack}
        canGoForward={active.canGoForward}
        filter={active.filter}
        deepSearch={active.deepSearch}
        {dualPane}
        bind:editing={breadcrumbEditing}
        onNavigate={navigateValidated}
        onBack={() => active.back()}
        onForward={() => active.forward()}
        onUp={() => active.up()}
        onFilterInput={handleFilterInput}
        onToggleDeep={toggleDeep}
        onToggleDual={() => (dualPane = !dualPane)}
        onClose={() => ipc.close()}
    />

    <div id="body">
        <Sidebar
            {pins}
            {drives}
            activePath={active.path}
            {ipc}
            onNavigate={navigateActive}
            onUnpin={(path) => void unpin(path)}
            onNavigateOther={navigateOther}
            onMove={handleMove}
        />

        <div id="panes">
            {#each dualPane ? [0, 1] : [0] as index (index)}
                {@render paneView(index)}
            {/each}
        </div>

        {#if previewEntry !== null}
            <PreviewPane entry={previewEntry} {ipc} />
        {/if}
    </div>

    <StatusBar
        itemCount={activeVisible.length}
        selectionCount={active.selection.size}
        {selectionBytes}
        driveLabel={activeDrive?.label ?? ''}
        driveFree={activeDrive !== null ? formatSize(activeDrive.free_bytes) : ''}
    />
</div>

<Toasts />

{#if propertiesTarget !== null}
    <PropertiesModal target={propertiesTarget} onClose={() => (propertiesTarget = null)} />
{/if}

{#if promptRequest !== null}
    <PromptModal
        title={promptRequest.title}
        initial={promptRequest.initial}
        onSubmit={submitPrompt}
        onCancel={() => (promptRequest = null)}
    />
{/if}

{#if confirmRequest !== null}
    <ConfirmModal
        title={confirmRequest.title}
        message={confirmRequest.message}
        confirmLabel={confirmRequest.confirmLabel}
        onConfirm={() => {
            const request = confirmRequest;
            confirmRequest = null;
            request?.onConfirm();
        }}
        onCancel={() => (confirmRequest = null)}
    />
{/if}

{#snippet paneView(index: number)}
    {@const pane = panes[index]}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div
        class="pane"
        class:inactive-pane={activePaneIndex !== index}
        onpointerdowncapture={() => (activePaneIndex = index)}
        oncontextmenu={(event) => handleBackgroundContextMenu(index, event)}
        role="group"
        tabindex="-1"
    >
        <div class="pane-path">{pane.path}</div>
        <div class="cols">
            <button
                type="button"
                class="c-name"
                class:sorted={pane.sortBy === 'name'}
                onclick={() => sortColumn(pane, 'name')}>Name</button
            >
            <button
                type="button"
                class="c-size"
                class:sorted={pane.sortBy === 'size'}
                onclick={() => sortColumn(pane, 'size')}>Size</button
            >
            <button
                type="button"
                class="c-mtime"
                class:sorted={pane.sortBy === 'mtime'}
                onclick={() => sortColumn(pane, 'mtime')}>Modified</button
            >
        </div>
        <FileList
            entries={pane.visibleEntries()}
            selection={pane.selection}
            maxSize={maxEntrySize(pane.entries)}
            path={pane.path}
            onMove={handleMove}
            onSelect={(path, event) => handleSelect(index, path, event)}
            onOpen={(entry) => openEntry(index, entry)}
            onContextMenu={(entry, event) => handleEntryContextMenu(index, entry, event)}
        />
    </div>
{/snippet}

<style>
    #win {
        height: 100%;
        display: flex;
        flex-direction: column;
        background: var(--color-bg);
        color: var(--color-fg);
        overflow: hidden;
        min-height: 0;
    }
    #body {
        flex: 1;
        display: flex;
        min-height: 0;
    }
    #panes {
        flex: 1;
        display: flex;
        min-width: 0;
    }
    .pane {
        flex: 1;
        display: flex;
        flex-direction: column;
        min-width: 0;
        outline: none;
        position: relative;
    }
    .pane + .pane {
        border-left: 1px solid var(--color-border);
    }
    .pane.inactive-pane {
        opacity: 0.75;
    }
    .pane-path {
        flex: none;
        font-size: 10px;
        font-family: var(--font-mono, ui-monospace, monospace);
        color: var(--color-muted);
        padding: 4px 12px;
        border-bottom: 1px solid var(--color-border);
        display: none;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    #win.dual .pane-path {
        display: block;
    }
    .cols {
        flex: none;
        display: flex;
        align-items: center;
        padding: 0 12px;
        height: 26px;
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        color: var(--color-muted);
        border-bottom: 1px solid var(--color-border);
        user-select: none;
    }
    .cols button {
        background: transparent;
        border: none;
        padding: 0;
        font: inherit;
        text-transform: inherit;
        letter-spacing: inherit;
        color: inherit;
        cursor: pointer;
    }
    .cols button:hover {
        color: var(--color-fg-alt);
    }
    .cols .c-name {
        flex: 1;
    }
    .cols .c-size {
        width: 130px;
        text-align: right;
    }
    .cols .c-mtime {
        width: 130px;
        text-align: right;
    }
    .cols .sorted {
        color: var(--color-accent);
    }
</style>
