<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { ActiveWindowState, MonitorActiveWindowState } from '../lib/types';
    import { ACTIVE_WINDOW_CHANNEL, ACTIVE_WINDOW_PROVIDER } from './channels';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let payload: MonitorActiveWindowState = $state({ monitors: {}, focused_monitor: null });

    // `$effect` runs reliably in testing-library + Svelte 5 where `onMount` does not.
    $effect(() => {
        client.call('provider.query', { id: ACTIVE_WINDOW_PROVIDER })
            .then((r: unknown) => { if (r) payload = r as MonitorActiveWindowState; })
            .catch(() => {});
        const unsubscribe = client.subscribe(ACTIVE_WINDOW_CHANNEL, (p: unknown) => {
            payload = p as MonitorActiveWindowState;
        });
        return () => unsubscribe?.();
    });

    function selectMonitor(p: MonitorActiveWindowState): string | null {
        return window.__quantum_monitor ?? p.focused_monitor ?? null;
    }

    let state: ActiveWindowState | null = $derived.by(() => {
        const selected = selectMonitor(payload);
        if (!selected) return null;
        return payload.monitors[selected] ?? null;
    });

    function displayTitle(s: ActiveWindowState | null): string {
        if (!s) return '';
        const text = s.title.trim() || s.class.trim();
        return text;
    }

    function displayWorkspace(s: ActiveWindowState | null): string | null {
        if (!s) return null;
        if (s.workspace_name.startsWith('special:')) return null;
        const text = s.workspace_name || String(s.workspace_id);
        // Numeric workspace names get a '#' prefix to match the bar's
        // typographic style. Named workspaces (e.g. 'web', 'chat') are
        // shown verbatim.
        return /^\d+$/.test(text) ? `#${text}` : text;
    }
</script>

<div class="active-window">
    {#if displayWorkspace(state)}
        <span class="workspace">{displayWorkspace(state)}</span>
        <span class="separator" aria-hidden="true">·</span>
    {/if}
    <span class="title">{displayTitle(state)}</span>
</div>

<style>
    .active-window {
        display: flex;
        align-items: baseline;
        gap: 8px;
        min-width: 0;
        overflow: hidden;
        font-size: 14px;
        line-height: 1;
    }
    /* Workspace label: a dimmer typographic accent. Tabular nums so
       "#1" and "#10" align. No background, no pill — the hash
       prefix is enough to mark it as a workspace tag. */
    .workspace {
        color: var(--color-accent, #89b4fa);
        font-weight: 600;
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
        opacity: 0.85;
        flex-shrink: 0;
    }
    /* Discreet middle-dot between workspace and title. Same accent
       tint as the workspace so the two read as a single label group. */
    .separator {
        color: var(--color-accent, #89b4fa);
        opacity: 0.5;
        flex-shrink: 0;
    }
    .title {
        color: var(--color-fg, #cdd6f4);
        font-weight: 500;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        min-width: 0;
    }
</style>
