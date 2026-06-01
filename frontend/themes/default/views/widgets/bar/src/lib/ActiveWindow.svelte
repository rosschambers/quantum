<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { ActiveWindowState } from '../lib/types';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: ActiveWindowState | null = $state(null);

    // `$effect` runs reliably in testing-library + Svelte 5 where `onMount` does not.
    $effect(() => {
        client.call('provider.query', { id: 'hyprland.activewindow' })
            .then((r: unknown) => { if (r) state = r as ActiveWindowState; })
            .catch(() => {});
        const unsubscribe = client.subscribe('hyprland.activewindow.event', (payload: unknown) => {
            state = payload as ActiveWindowState;
        });
        return () => unsubscribe?.();
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
        <span class="workspace">{displayWorkspace(state)}:</span>
    {/if}
    <span class="title">{displayTitle(state)}</span>
</div>

<style>
    .active-window {
        display: flex;
        align-items: baseline;
        gap: 6px;
        min-width: 0;
        overflow: hidden;
    }
    /*
     * Workspace number is shown as plain muted text — no pill, no
     * border. Sits inline with the title so the whole left region
     * reads as a single label like "1: firefox — mozilla.org".
     */
    .workspace {
        font-size: var(--font-size-sm, 12px);
        color: var(--color-fg-alt, #a6adc8);
        font-variant-numeric: tabular-nums;
    }
    .title {
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        color: var(--color-fg, #cdd6f4);
    }
</style>
