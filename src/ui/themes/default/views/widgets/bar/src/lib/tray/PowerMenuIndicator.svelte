<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { SystemPowerState } from '../types';
    import { SYSTEM_POWER_CHANNEL, SYSTEM_POWER_PROVIDER } from '../channels';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';

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
        // The bar widget is injected with a per-monitor `__quantum_monitor`
        // global (e.g. "DP-1") by the WebView host. Append it as an `@monitor`
        // suffix so the registry pins the power-menu overlay to the same
        // monitor as the bar that triggered it.
        const monitor = window.__quantum_monitor;
        const name = monitor
            ? `widgets/power-menu@${monitor}`
            : 'widgets/power-menu';
        try {
            await client.call('view.show', { name });
        } catch (err) {
            console.error(`view.show ${name} failed:`, err);
        }
    }
</script>

{#if anyCapable(state)}
    <BarButton ariaLabel="Power menu" onclick={openMenu}>
        <Icon name="power" size={18} />
    </BarButton>
{/if}
