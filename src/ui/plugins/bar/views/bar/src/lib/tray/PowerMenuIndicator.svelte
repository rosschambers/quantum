<script lang="ts">
    import type { Client, MenuItem } from '@quantum/client';
    import type { SystemPowerState } from '../types';
    import { SYSTEM_POWER_CHANNEL, SYSTEM_POWER_PROVIDER } from '../channels';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';
    import { monitorView, wireBarMenu } from './barMenu';

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
    let buttonEl: HTMLButtonElement | undefined = $state(undefined);

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

    // Right-click opens quick power actions, each gated on the matching
    // capability so unavailable actions never appear.
    $effect(() => {
        const node = buttonEl;
        if (!node) return;
        return wireBarMenu(node, client, buildMenuItems);
    });

    function invoke(command: string): Promise<unknown> {
        return client.call('action.invoke', {
            provider: 'system_power',
            action: {
                kind: 'custom',
                data: { kind: 'system_power', payload: { command } },
            },
        });
    }

    function powerAction(command: string): () => void {
        return () =>
            invoke(command).catch((err) =>
                console.error(`system_power ${command} failed:`, err),
            );
    }

    function buildMenuItems(): MenuItem[] {
        const items: MenuItem[] = [];
        if (state.can_lock) {
            items.push({ label: 'Lock', onSelect: powerAction('lock') });
        }
        if (state.can_suspend) {
            items.push({ label: 'Suspend', onSelect: powerAction('suspend') });
        }
        if (state.can_hibernate) {
            items.push({ label: 'Hibernate', onSelect: powerAction('hibernate') });
        }
        items.push({ separator: true, label: '' });
        if (state.can_restart) {
            items.push({ label: 'Restart', onSelect: powerAction('restart') });
        }
        if (state.can_shutdown) {
            items.push({ label: 'Shutdown', danger: true, onSelect: powerAction('shutdown') });
        }
        items.push({ separator: true, label: '' });
        items.push({ label: 'Power menu...', onSelect: openMenu });
        return items;
    }

    async function openMenu(): Promise<void> {
        // The bar widget is injected with a per-monitor `__quantum_monitor`
        // global (e.g. "DP-1") by the WebView host. Append it as an `@monitor`
        // suffix so the registry pins the power-menu overlay to the same
        // monitor as the bar that triggered it.
        const name = monitorView('widgets/power-menu');
        try {
            await client.call('view.show', { name });
        } catch (err) {
            console.error(`view.show ${name} failed:`, err);
        }
    }
</script>

{#if anyCapable(state)}
    <BarButton ariaLabel="Power menu" onclick={openMenu} bindRef={(el) => (buttonEl = el)}>
        <Icon name="power" size={18} />
    </BarButton>
{/if}
