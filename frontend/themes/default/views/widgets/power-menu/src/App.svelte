<script lang="ts">
    import { createClient } from '@quantum/client';
    import type { SystemPowerState } from './lib/types';

    const client = createClient();
    let state: SystemPowerState = $state({
        can_shutdown: false,
        can_restart: false,
        can_suspend: false,
        can_hibernate: false,
        can_lock: false,
    });

    $effect(() => {
        client
            .call('provider.query', { id: 'system_power' })
            .then((r: unknown) => {
                if (r) state = r as SystemPowerState;
            })
            .catch(() => {});
        const off = client.subscribe('system_power.event', (p: unknown) => {
            state = p as SystemPowerState;
        });
        return () => {
            off?.();
            client.close();
        };
    });

    /**
     * Send `view.hide` so the panel-shell window closes without
     * needing the daemon to hold the window open after the user
     * dismisses it. Fire-and-forget; the worst failure mode is "the
     * panel stays open after backdrop click", which is no worse than
     * any other transient IPC failure.
     */
    function close(): void {
        client.call('view.hide', { name: 'widgets/power-menu' }).catch(() => {});
    }

    function onBackdropClick(event: MouseEvent): void {
        // Only fire when the actual backdrop element (not a descendant
        // like the card) was clicked.
        if (event.target === event.currentTarget) close();
    }

    function onKeyDown(event: KeyboardEvent): void {
        if (event.key === 'Escape') {
            event.preventDefault();
            close();
        }
    }

    $effect(() => {
        document.addEventListener('keydown', onKeyDown);
        return () => document.removeEventListener('keydown', onKeyDown);
    });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="Power menu">
        <header class="card-header">
            <h2>Power</h2>
            <button type="button" class="close" aria-label="Close" onclick={close}>×</button>
        </header>
        <!--
            Row placeholders. Tasks 3.3-3.5 swap these out for real
            ActionRow / ScheduleExpander / ScheduledList components.
        -->
        {#if state.can_shutdown}<div class="row">Shutdown</div>{/if}
        {#if state.can_restart}<div class="row">Restart</div>{/if}
        {#if state.can_suspend}<div class="row">Suspend</div>{/if}
        {#if state.can_hibernate}<div class="row">Hibernate</div>{/if}
        {#if state.can_lock}<div class="row">Lock</div>{/if}
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.5);
        backdrop-filter: blur(4px);
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .card {
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border-radius: 10px;
        padding: 16px 20px;
        min-width: 440px;
        box-shadow: 0 10px 30px rgba(0, 0, 0, 0.6);
        border: 1px solid var(--color-border, #45475a);
    }
    .card-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        margin-bottom: 8px;
    }
    h2 {
        margin: 0;
        font-size: 14px;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        color: var(--color-fg-alt, #a6adc8);
        font-weight: 600;
    }
    .close {
        background: transparent;
        border: none;
        color: var(--color-fg-alt, #a6adc8);
        font-size: 18px;
        cursor: pointer;
        line-height: 1;
        padding: 2px 6px;
        border-radius: 4px;
    }
    .close:hover {
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
    }
    .row {
        padding: 10px 12px;
        border-radius: 6px;
        cursor: pointer;
        margin: 2px 0;
        font-size: 14px;
    }
    .row:hover {
        background: var(--color-bg, #1e1e2e);
    }
</style>
