<script lang="ts">
    import { createNotificationStore, type Client } from '@quantum/client';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let count = $state(0);

    $effect(() => {
        const off = createNotificationStore(client).subscribe((list) => {
            count = list.length;
        });
        return () => off?.();
    });

    function badgeLabel(n: number): string {
        return n > 9 ? '9+' : String(n);
    }

    async function openCenter(): Promise<void> {
        // The bar widget is injected with a per-monitor `__quantum_monitor`
        // global (e.g. "DP-1") by the WebView host. Append it as an
        // `@monitor` suffix so the registry pins the notification center to
        // the same monitor as the bar that triggered it.
        const monitor = window.__quantum_monitor;
        const name = monitor
            ? `plugin/notification-center/center@${monitor}`
            : 'plugin/notification-center/center';
        try {
            await client.call('view.toggle', { name });
        } catch (err) {
            console.error(`view.toggle ${name} failed:`, err);
        }
        // Opening the center means the user is now reading their notifications,
        // so clear the transient on-screen toasts. This is display-only: the
        // notifications stay in the center and the bell count is unchanged.
        try {
            await client.call('action.invoke', {
                provider: 'notifications',
                action: {
                    kind: 'custom',
                    data: { kind: 'notifications', payload: { command: 'clear_toasts' } },
                },
            });
        } catch (err) {
            console.error('clear_toasts failed:', err);
        }
    }
</script>

<BarButton ariaLabel="Notifications" onclick={openCenter}>
    <span class="notification-icon">
        <Icon name="bell" size={18} />
        {#if count > 0}
            <span class="notification-badge">{badgeLabel(count)}</span>
        {/if}
    </span>
</BarButton>

<style>
    .notification-icon {
        position: relative;
        display: inline-flex;
        align-items: center;
        justify-content: center;
    }
    .notification-badge {
        position: absolute;
        top: -6px;
        right: -8px;
        min-width: 14px;
        height: 14px;
        padding: 0 3px;
        box-sizing: border-box;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 7px;
        background: var(--color-accent, #f38ba8);
        color: var(--color-bg, #1e1e2e);
        font-size: 10px;
        line-height: 1;
        font-weight: 600;
    }
</style>
