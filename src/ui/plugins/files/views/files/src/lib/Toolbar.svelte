<script lang="ts">
    /**
     * The top toolbar: back / forward / up icon buttons, the embedded path
     * breadcrumb, a filter box with a recursive "deep" toggle, and a dual-pane
     * toggle. Per the settled design this DROPS the playground's list/grid and
     * hidden-files toggles — the list is always a detailed list and hidden
     * files are always shown — so the only trailing control is the dual-pane
     * toggle. Every icon button carries a native title tooltip (house style).
     */
    import Icon from './Icon.svelte';
    import Breadcrumb from './Breadcrumb.svelte';

    interface Props {
        /** The active pane's current path, forwarded to the breadcrumb. */
        path: string;
        /** Whether the active pane has back history. */
        canGoBack: boolean;
        /** Whether the active pane has forward history. */
        canGoForward: boolean;
        /** The active pane's filter text. */
        filter: string;
        /** Whether recursive (deep) search is on for the active pane. */
        deepSearch: boolean;
        /** Whether the second pane is shown. */
        dualPane: boolean;
        onNavigate: (path: string) => void;
        onBack: () => void;
        onForward: () => void;
        onUp: () => void;
        onFilterInput: (value: string) => void;
        onToggleDeep: () => void;
        onToggleDual: () => void;
    }

    let {
        path,
        canGoBack,
        canGoForward,
        filter,
        deepSearch,
        dualPane,
        onNavigate,
        onBack,
        onForward,
        onUp,
        onFilterInput,
        onToggleDeep,
        onToggleDual,
    }: Props = $props();

    // Up is enabled everywhere except the filesystem root.
    const canGoUp = $derived(path !== '/');

    function handleFilterInput(event: Event): void {
        onFilterInput((event.currentTarget as HTMLInputElement).value);
    }
</script>

<div class="toolbar">
    <button
        type="button"
        class="icon-btn b-back"
        title="Back (Alt+Left)"
        disabled={!canGoBack}
        onclick={onBack}
    >
        <Icon name="back" size={16} />
    </button>
    <button
        type="button"
        class="icon-btn b-forward"
        title="Forward (Alt+Right)"
        disabled={!canGoForward}
        onclick={onForward}
    >
        <Icon name="forward" size={16} />
    </button>
    <button
        type="button"
        class="icon-btn b-up"
        title="Parent folder (Alt+Up)"
        disabled={!canGoUp}
        onclick={onUp}
    >
        <Icon name="up" size={16} />
    </button>

    <Breadcrumb {path} {onNavigate} />

    <div class="filter-wrap">
        <span class="filter-ic"><Icon name="search" size={13} /></span>
        <input
            class="filter-input"
            placeholder="Filter..."
            title="Filter this folder; toggle deep for recursive search"
            value={filter}
            oninput={handleFilterInput}
        />
        <button
            type="button"
            class="deep"
            class:on={deepSearch}
            title="Search recursively below this folder"
            onclick={onToggleDeep}
        >
            deep
        </button>
    </div>

    <button
        type="button"
        class="icon-btn b-dual"
        class:active={dualPane}
        title="Toggle dual pane"
        onclick={onToggleDual}
    >
        <Icon name="columns" size={16} />
    </button>
</div>

<style>
    .toolbar {
        height: 42px;
        flex: none;
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 0 10px;
        border-bottom: 1px solid var(--color-border);
    }
    .icon-btn {
        width: 28px;
        height: 28px;
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: transparent;
        border: none;
        border-radius: 7px;
        color: var(--color-fg-alt);
        cursor: pointer;
    }
    .icon-btn:hover {
        background: var(--color-surface-hover);
    }
    .icon-btn.active {
        background: rgba(166, 227, 161, 0.18);
        color: var(--color-accent);
    }
    .icon-btn:disabled {
        opacity: 0.35;
        cursor: default;
    }
    .icon-btn:disabled:hover {
        background: transparent;
    }
    .filter-wrap {
        display: flex;
        align-items: center;
        gap: 4px;
        background: var(--color-bg-alt);
        border: 1px solid var(--color-border);
        border-radius: 8px;
        height: 30px;
        padding: 0 8px;
        width: 200px;
    }
    .filter-ic {
        display: inline-flex;
        align-items: center;
        color: var(--color-muted);
        flex: none;
    }
    .filter-input {
        flex: 1;
        min-width: 0;
        background: transparent;
        border: none;
        outline: none;
        color: var(--color-fg);
        font-size: 12px;
    }
    .deep {
        font-size: 9px;
        font-family: inherit;
        padding: 2px 5px;
        border-radius: 4px;
        cursor: pointer;
        user-select: none;
        color: var(--color-muted);
        background: transparent;
        border: 1px solid var(--color-border);
    }
    .deep.on {
        color: var(--color-accent);
        border-color: var(--color-accent);
    }
</style>
