<script lang="ts">
    import { createClient } from '@quantum/client';
    import type { IconName } from './lib/Icon.svelte';
    import ActionTile from './lib/ActionTile.svelte';
    import DelayPicker from './lib/DelayPicker.svelte';
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
     * Which tile is currently armed for two-click confirmation. Lifted
     * to the parent so only one tile can arm at a time. Disarms after
     * CONFIRM_WINDOW_MS without a second click.
     */
    let armed: PowerCommand | null = $state(null);
    let disarmTimeout: ReturnType<typeof setTimeout> | null = null;
    const CONFIRM_WINDOW_MS = 3000;

    /**
     * Single global delay applied to whichever tile the user invokes
     * next. `0` = invoke immediately via `action.invoke`; any positive
     * value = `action.schedule` with that delay in seconds. Owned at
     * the parent so it survives armed state changes.
     */
    let delaySecs: number = $state(0);

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
     * First-click handler from any ActionTile. Sets `armed`, clears
     * any pending disarm timer, and schedules a new disarm after
     * CONFIRM_WINDOW_MS so the tile reverts on inaction.
     */
    function handleArm(command: PowerCommand): void {
        clearDisarm();
        armed = command;
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
     * Second-click confirmation. Routes to `action.invoke` when the
     * delay picker is on Now (delaySecs === 0); otherwise routes to
     * `action.schedule` with the picker's delay. Closes the menu on
     * immediate invoke; stays open after a schedule so the user can
     * see the new entry in the footer list.
     */
    async function invoke(command: PowerCommand, label: string): Promise<void> {
        clearDisarm();
        armed = null;
        if (delaySecs === 0) {
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
            return;
        }
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

    function onDelayChange(secs: number): void {
        delaySecs = secs;
    }

    /**
     * Mapping table from command to display label + icon + tier.
     * Kept here so adding/changing actions is a one-place edit.
     * Primary tier = shutdown, restart. Secondary tier = suspend,
     * hibernate, lock.
     */
    interface ActionDef {
        command: PowerCommand;
        label: string;
        icon: IconName;
        variant: 'primary' | 'secondary';
        capable: boolean;
    }

    const allActions: ActionDef[] = $derived([
        {
            command: 'shutdown',
            label: 'Shutdown',
            icon: 'power',
            variant: 'primary',
            capable: state.can_shutdown,
        },
        {
            command: 'restart',
            label: 'Restart',
            icon: 'balanced',
            variant: 'primary',
            capable: state.can_restart,
        },
        {
            command: 'suspend',
            label: 'Suspend',
            icon: 'sun',
            variant: 'secondary',
            capable: state.can_suspend,
        },
        {
            command: 'hibernate',
            label: 'Hibernate',
            icon: 'leaf',
            variant: 'secondary',
            capable: state.can_hibernate,
        },
        {
            command: 'lock',
            label: 'Lock',
            icon: 'bolt',
            variant: 'secondary',
            capable: state.can_lock,
        },
    ]);

    const primaryActions = $derived(
        allActions.filter((action) => action.variant === 'primary' && action.capable),
    );
    const secondaryActions = $derived(
        allActions.filter((action) => action.variant === 'secondary' && action.capable),
    );
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="Power menu">
        {#if primaryActions.length > 0}
            <div class="tier tier-primary">
                {#each primaryActions as action (action.command)}
                    <ActionTile
                        command={action.command}
                        label={action.label}
                        icon={action.icon}
                        variant={action.variant}
                        {armed}
                        onArm={handleArm}
                        onInvoke={() => invoke(action.command, action.label)}
                    />
                {/each}
            </div>
        {/if}

        {#if secondaryActions.length > 0}
            <div
                class="tier tier-secondary"
                class:divider={primaryActions.length > 0}
            >
                {#each secondaryActions as action (action.command)}
                    <ActionTile
                        command={action.command}
                        label={action.label}
                        icon={action.icon}
                        variant={action.variant}
                        {armed}
                        onArm={handleArm}
                        onInvoke={() => invoke(action.command, action.label)}
                    />
                {/each}
            </div>
        {/if}

        <ScheduledList jobs={scheduled} onCancel={cancelScheduled} />

        <DelayPicker value={delaySecs} onChange={onDelayChange} />
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        background: var(--color-overlay-backdrop, rgba(0, 0, 0, 0.5));
        backdrop-filter: blur(4px);
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .card {
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border-radius: 12px;
        padding: 18px 20px;
        width: min(420px, 92vw);
        max-height: 90vh;
        overflow: auto;
        box-sizing: border-box;
        box-shadow: 0 14px 40px var(--color-shadow, rgba(0, 0, 0, 0.6));
        border: 1px solid var(--color-border, #45475a);
    }
    .tier-primary {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 12px;
    }
    /* Cap the tile width so a primary tile is never wider than tall
       (square-ish). When the card is at its 420px width minus padding
       and gap, each column is ~184px; without this cap the tile
       stretches as the card stretches and looks empty. */
    .tier-primary :global(.action-tile) {
        max-width: 200px;
        margin: 0 auto;
        width: 100%;
    }
    .tier-secondary {
        display: flex;
        gap: 6px;
        justify-content: space-around;
        margin-top: 12px;
    }
    .tier-secondary.divider {
        padding-top: 14px;
        border-top: 1px solid var(--color-border, #45475a);
    }
    .tier-secondary :global(.action-tile) {
        flex: 1;
        max-width: 140px;
    }
</style>
