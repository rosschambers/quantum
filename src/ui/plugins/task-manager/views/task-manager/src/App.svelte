<script lang="ts">
    import {
        createClient,
        PROCESSES_EVENT_CHANNEL,
        PROCESSES_WATCH,
        PROCESSES_UNWATCH,
        type Client,
        type ProcessSnapshot,
    } from '@quantum/client';
    import Icon from './lib/Icon.svelte';

    /**
     * The IPC client is injected so tests can pass a stub with spies. In the
     * running panel it defaults to the bridge-backed client, matching the
     * dependency-injection shape the files view uses for its `ipc` prop.
     */
    interface Props {
        client?: Client;
    }

    let { client = createClient() }: Props = $props();

    /** The canonical view name the compositor registers this panel under. */
    const VIEW_NAME = 'plugin/task-manager/task-manager';

    /** Latest process snapshot pushed on the processes channel; null until the first arrives. */
    let snapshot: ProcessSnapshot | null = $state(null);

    /** Titlebar filter text. Filtering logic lands in a later task; this only holds the value. */
    let filterText = $state('');

    /**
     * Count of top-level entries in the latest snapshot. Proves the
     * subscription is live before the tree view exists; the real tree
     * replaces this placeholder in a later task.
     */
    const processCount = $derived(
        snapshot === null ? null : snapshot.apps.length + snapshot.background.length,
    );

    // Process subscription lifecycle. Start the watch, then subscribe to the
    // snapshot channel. `destroy_on_dismiss` makes this cleanup unreliable on
    // dismiss, so `close()` also stops the watch explicitly (mirroring how the
    // session-owning overlays send their close command before `view.hide`);
    // this cleanup remains the best-effort path for a plain unmount.
    $effect(() => {
        client.call(PROCESSES_WATCH, {}).catch(() => {});
        const unsubscribe = client.subscribe(PROCESSES_EVENT_CHANNEL, (payload: unknown) => {
            snapshot = payload as ProcessSnapshot;
        });
        return () => {
            unsubscribe?.();
            client.call(PROCESSES_UNWATCH, {}).catch(() => {});
        };
    });

    // Window-level Escape: clear a non-empty filter first, otherwise close.
    // Matches the files panel, which listens on `window` (not `document`).
    $effect(() => {
        window.addEventListener('keydown', onKeyDown);
        return () => window.removeEventListener('keydown', onKeyDown);
    });

    function onKeyDown(event: KeyboardEvent): void {
        if (event.key !== 'Escape') return;
        event.preventDefault();
        if (filterText !== '') {
            filterText = '';
            return;
        }
        close();
    }

    /**
     * Stop the process watch, then hide the panel by its canonical name.
     * The watch is stopped here (not only in the `$effect` cleanup) because
     * `destroy_on_dismiss` tears the webview down without a reliable unmount,
     * which would otherwise leave the daemon sampling processes with no viewer.
     */
    function close(): void {
        client.call(PROCESSES_UNWATCH, {}).catch(() => {});
        client.call('view.hide', { name: VIEW_NAME }).catch(() => {});
    }
</script>

<div class="panel">
    <div class="titlebar">
        <span class="dot"></span>
        <span class="title">Task Manager</span>
        <div class="searchbox">
            <Icon name="search" size={13} />
            <input
                class="filter-input"
                type="text"
                placeholder="Filter processes…"
                autocomplete="off"
                aria-label="Filter processes"
                bind:value={filterText}
            />
        </div>
        <button type="button" class="closex" title="Close" aria-label="Close" onclick={close}>
            <Icon name="close" size={14} />
        </button>
    </div>
    <div class="body">
        {#if processCount === null}
            Loading…
        {:else}
            {processCount} top-level processes
        {/if}
    </div>
</div>

<style>
    .panel {
        height: 100%;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        background: var(--color-bg, #292520);
        color: var(--color-fg, #e8e4d8);
    }
    .titlebar {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 6px 8px 6px 14px;
        background: var(--color-bg-alt, #3b3630);
        border-bottom: 1px solid var(--color-border, #4a443e);
        font-size: 13px;
        color: var(--color-fg-alt, #d8d4c8);
        flex: none;
    }
    .dot {
        width: 10px;
        height: 10px;
        border-radius: 50%;
        background: var(--color-muted, #8a8578);
        opacity: 0.5;
        flex: none;
    }
    .title {
        flex: 1;
        font-weight: 600;
    }
    .searchbox {
        display: flex;
        align-items: center;
        gap: 6px;
        background: var(--color-bg, #292520);
        border: 1px solid var(--color-border, #4a443e);
        border-radius: 8px;
        padding: 3px 9px;
        width: 230px;
        color: var(--color-muted, #8a8578);
    }
    .searchbox :global(.icon) {
        flex: none;
    }
    .filter-input {
        flex: 1;
        background: none;
        border: 0;
        outline: 0;
        color: var(--color-fg, #e8e4d8);
        font-size: 12.5px;
        font-family: var(--font-sans, system-ui, sans-serif);
        min-width: 0;
    }
    .filter-input::placeholder {
        color: var(--color-muted, #8a8578);
    }
    .closex {
        background: none;
        border: 0;
        color: var(--color-muted, #8a8578);
        cursor: pointer;
        width: 26px;
        height: 26px;
        border-radius: 7px;
        display: flex;
        align-items: center;
        justify-content: center;
        margin-left: 4px;
    }
    .closex:hover {
        background: var(--color-error, #af4e3c);
        color: var(--color-fg, #e8e4d8);
    }
    .body {
        flex: 1;
        overflow-y: auto;
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--color-muted, #8a8578);
        font-size: 13px;
    }
</style>
