<script lang="ts">
    import { createClient } from '@quantum/client';
    import type { IconName } from './lib/Icon.svelte';
    import ActionRow from './lib/ActionRow.svelte';
    import ScheduleExpander from './lib/ScheduleExpander.svelte';
    import ScheduledList from './lib/ScheduledList.svelte';
    import type { PowerCommand, ScheduledJob, SystemPowerState } from './lib/types';

    const client = createClient();

    let state: SystemPowerState = $state({
        can_shutdown: false,
        can_restart: false,
        can_suspend: false,
        can_hibernate: false,
        can_lock: false,
    });

    /**
     * Which row is currently armed for two-click confirmation. Lifted
     * to the parent so only one row can arm at a time. Disarms after
     * CONFIRM_WINDOW_MS without a second click.
     */
    let armed: PowerCommand | null = $state(null);
    let disarmTimeout: ReturnType<typeof setTimeout> | null = null;
    const CONFIRM_WINDOW_MS = 3000;

    /**
     * Which row has its schedule expander open. At most one. The
     * expander itself doesn't own this state \u2014 same lift-up pattern as
     * `armed` \u2014 so opening one row's expander auto-closes any other.
     */
    let expanded: PowerCommand | null = $state(null);

    let scheduled: ScheduledJob[] = $state([]);

    /**
     * Capability snapshot + live updates. Also kicks off the initial
     * scheduled-jobs fetch.
     */
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
        void refreshScheduled();
        return () => {
            off?.();
            if (disarmTimeout !== null) clearTimeout(disarmTimeout);
            client.close();
        };
    });

    $effect(() => {
        document.addEventListener('keydown', onKeyDown);
        return () => document.removeEventListener('keydown', onKeyDown);
    });

    function onKeyDown(event: KeyboardEvent): void {
        if (event.key === 'Escape') {
            event.preventDefault();
            close();
        }
    }

    function onBackdropClick(event: MouseEvent): void {
        if (event.target === event.currentTarget) close();
    }

    function close(): void {
        client.call('view.hide', { name: 'widgets/power-menu' }).catch(() => {});
    }

    /**
     * First-click handler from any ActionRow. Sets `armed`, clears
     * any pending disarm timer, and schedules a new disarm after
     * CONFIRM_WINDOW_MS so the row reverts on inaction.
     */
    function handleArm(command: PowerCommand): void {
        clearDisarm();
        armed = command;
        // Closing any open expander when arming \u2014 the user's intent is
        // clearly the immediate-action path.
        expanded = null;
        disarmTimeout = setTimeout(() => {
            if (armed === command) armed = null;
            disarmTimeout = null;
        }, CONFIRM_WINDOW_MS);
    }

    function clearDisarm(): void {
        if (disarmTimeout !== null) {
            clearTimeout(disarmTimeout);
            disarmTimeout = null;
        }
    }

    /**
     * Second-click confirmation. Fires the immediate-action path
     * (action.invoke). Closes the menu after success since the system
     * is about to shut down / sleep / etc.
     */
    async function invokeNow(command: PowerCommand): Promise<void> {
        clearDisarm();
        armed = null;
        try {
            await client.call('action.invoke', {
                provider: 'system_power',
                action: {
                    kind: 'custom',
                    data: { kind: 'system_power', payload: { command } },
                },
            });
        } catch (err) {
            console.error(`system_power ${command} failed:`, err);
        }
        close();
    }

    /**
     * Schedule a delayed invocation of `command` after `delaySecs`.
     * Stays open so the user can see the new entry in the footer.
     */
    async function schedule(command: PowerCommand, label: string, delaySecs: number): Promise<void> {
        clearDisarm();
        armed = null;
        try {
            await client.call('action.schedule', {
                delay_secs: delaySecs,
                label,
                action: {
                    provider: 'system_power',
                    action: {
                        kind: 'custom',
                        data: { kind: 'system_power', payload: { command } },
                    },
                },
            });
            await refreshScheduled();
        } catch (err) {
            console.error(`system_power ${command} schedule failed:`, err);
        }
    }

    async function refreshScheduled(): Promise<void> {
        try {
            const r = (await client.call('action.scheduled', {})) as { jobs?: ScheduledJob[] };
            scheduled = r?.jobs ?? [];
        } catch {
            scheduled = [];
        }
    }

    async function cancelScheduled(id: string): Promise<void> {
        try {
            await client.call('action.cancel', { id });
            await refreshScheduled();
        } catch (err) {
            console.error(`cancel scheduled job ${id} failed:`, err);
        }
    }

    function toggleExpanded(command: PowerCommand): void {
        expanded = expanded === command ? null : command;
        // Arming and expanding are mutually exclusive interactions.
        clearDisarm();
        armed = null;
    }

    /**
     * Mapping table from command to display label + icon. Kept here
     * so adding/changing actions is a one-place edit.
     */
    interface ActionDef {
        command: PowerCommand;
        label: string;
        icon: IconName;
        capable: boolean;
    }

    const actions: ActionDef[] = $derived([
        { command: 'shutdown', label: 'Shutdown', icon: 'power', capable: state.can_shutdown },
        { command: 'restart', label: 'Restart', icon: 'balanced', capable: state.can_restart },
        { command: 'suspend', label: 'Suspend', icon: 'sun', capable: state.can_suspend },
        { command: 'hibernate', label: 'Hibernate', icon: 'leaf', capable: state.can_hibernate },
        { command: 'lock', label: 'Lock', icon: 'bolt', capable: state.can_lock },
    ]);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="Power menu">
        <header class="card-header">
            <h2>Power</h2>
            <button type="button" class="close" aria-label="Close" onclick={close}>×</button>
        </header>

        {#each actions as action (action.command)}
            {#if action.capable}
                <div class="action-block">
                    <div class="row-and-toggle">
                        <ActionRow
                            command={action.command}
                            label={action.label}
                            icon={action.icon}
                            {armed}
                            onArm={handleArm}
                            onInvoke={() => invokeNow(action.command)}
                        />
                        <button
                            type="button"
                            class="expand-toggle"
                            class:open={expanded === action.command}
                            aria-label="Schedule {action.label}"
                            data-expand-command={action.command}
                            onclick={() => toggleExpanded(action.command)}
                        >Schedule…</button>
                    </div>
                    <ScheduleExpander
                        open={expanded === action.command}
                        onNow={() => handleArm(action.command)}
                        onSchedule={(delaySecs) => schedule(action.command, action.label, delaySecs)}
                        onClose={() => { expanded = null; }}
                    />
                </div>
            {/if}
        {/each}

        <ScheduledList jobs={scheduled} onCancel={cancelScheduled} />
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
        max-width: 520px;
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
        padding: 2px 8px;
        border-radius: 4px;
    }
    .close:hover {
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
    }
    .action-block {
        margin: 4px 0;
    }
    .row-and-toggle {
        display: flex;
        align-items: stretch;
        gap: 4px;
    }
    /*
     * ActionRow itself takes flex: 1 (button element with width: 100%
     * inside this flex parent). The schedule toggle is a fixed-width
     * pill at the right edge.
     */
    .row-and-toggle :global(.action-row) {
        flex: 1;
    }
    .expand-toggle {
        background: transparent;
        border: 1px solid transparent;
        color: var(--color-fg-alt, #a6adc8);
        font-size: 12px;
        cursor: pointer;
        border-radius: 6px;
        padding: 8px 12px;
        white-space: nowrap;
        align-self: center;
    }
    .expand-toggle:hover {
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
    }
    .expand-toggle.open {
        color: var(--color-accent, #89b4fa);
    }
</style>
