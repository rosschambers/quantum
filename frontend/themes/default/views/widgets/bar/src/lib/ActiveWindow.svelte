<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { ActiveWindowState, MonitorActiveWindowState } from '../lib/types';
    import { ACTIVE_WINDOW_CHANNEL } from './channels';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let payload: MonitorActiveWindowState = $state({ monitors: {}, focused_monitor: null });

    // `$effect` runs reliably in testing-library + Svelte 5 where `onMount` does not.
    $effect(() => {
        client.call('provider.query', { id: 'hyprland.activewindow' })
            .then((r: unknown) => { if (r) payload = r as MonitorActiveWindowState; })
            .catch(() => {});
        const unsubscribe = client.subscribe(ACTIVE_WINDOW_CHANNEL, (p: unknown) => {
            payload = p as MonitorActiveWindowState;
        });
        return () => unsubscribe?.();
    });

    function selectMonitor(p: MonitorActiveWindowState): string | null {
        return (window as any).__quantum_monitor ?? p.focused_monitor ?? null;
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
        return s.workspace_name || `${s.workspace_id}`;
    }
</script>

<div class="active-window">
    {#if displayWorkspace(state)}
        <span class="workspace">{displayWorkspace(state)}</span>
    {/if}
    <span class="title">{displayTitle(state)}</span>
</div>

<style>
    .active-window {
        display: flex;
        align-items: center;
        gap: var(--space-2, 0.5rem);
        min-width: 0;
        overflow: hidden;
    }
    .workspace {
        font-size: var(--font-size-sm, 12px);
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg-alt, #a6adc8);
        padding: 2px 8px;
        border-radius: var(--radius-sm, 2px);
    }
    .title {
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
</style>
