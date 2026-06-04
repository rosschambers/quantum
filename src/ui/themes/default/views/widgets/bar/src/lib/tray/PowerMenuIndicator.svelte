<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { SystemPowerState } from '../types';
    import { SYSTEM_POWER_CHANNEL, SYSTEM_POWER_PROVIDER } from '../channels';
    import Icon from '../Icon.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: SystemPowerState = $state({
        can_shutdown: false,
        can_restart: false,
        can_suspend: false,
        can_hibernate: false,
        can_lock: false,
    });

    $effect(() => {
        client
            .call('provider.query', { id: SYSTEM_POWER_PROVIDER })
            .then((r: unknown) => {
                if (r) state = r as SystemPowerState;
            })
            .catch(() => {});
        const off = client.subscribe(SYSTEM_POWER_CHANNEL, (payload: unknown) => {
            state = payload as SystemPowerState;
        });
        return () => off?.();
    });

    function anyCapable(s: SystemPowerState): boolean {
        return s.can_shutdown || s.can_restart || s.can_suspend || s.can_hibernate || s.can_lock;
    }

    async function openMenu(): Promise<void> {
        try {
            await client.call('view.show', { name: 'widgets/power-menu' });
        } catch (err) {
            console.error('view.show widgets/power-menu failed:', err);
        }
    }
</script>

{#if anyCapable(state)}
    <button
        type="button"
        class="tray-icon power-menu-trigger"
        aria-label="Power menu"
        onclick={openMenu}
    >
        <Icon name="power" size={14} />
    </button>
{/if}

<style>
    .tray-icon {
        background: transparent;
        border: none;
        padding: 0 4px;
        cursor: pointer;
        user-select: none;
        color: var(--color-fg-alt, #a6adc8);
        display: inline-flex;
        align-items: center;
        line-height: 1;
    }
    .tray-icon:hover {
        color: var(--color-fg, #cdd6f4);
    }
</style>
