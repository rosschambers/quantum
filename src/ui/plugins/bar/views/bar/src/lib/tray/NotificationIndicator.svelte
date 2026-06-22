<script lang="ts">
    import {
        createNotificationStore,
        type Client,
        type MenuItem,
        type PendingNotification,
    } from '@quantum/client';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';
    import { monitorView, wireBarMenu } from './barMenu';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let notifications: PendingNotification[] = $state([]);
    let buttonEl: HTMLButtonElement | undefined = $state(undefined);
    let count = $derived(notifications.length);

    $effect(() => {
        const off = createNotificationStore(client).subscribe((list) => {
            notifications = list;
        });
        return () => off?.();
    });

    // Right-click opens quick actions built from the current notification list.
    $effect(() => {
        const node = buttonEl;
        if (!node) return;
        return wireBarMenu(node, client, buildMenuItems);
    });

    function buildMenuItems(): MenuItem[] {
        return [
            {
                label: `Dismiss all (${count})`,
                disabled: count === 0,
                onSelect: dismissAll,
            },
            { label: 'Clear toasts', onSelect: clearToasts },
            { separator: true, label: '' },
            { label: 'Open notifications', onSelect: openCenter },
        ];
    }

    function invoke(payload: Record<string, unknown>): Promise<unknown> {
        return client.call('action.invoke', {
            provider: 'notifications',
            action: {
                kind: 'custom',
                data: { kind: 'notifications', payload },
            },
        });
    }

    function dismissAll(): void {
        // Snapshot the list first so concurrent updates do not skip ids, the
        // same way the notification center dismisses everything.
        const ids = notifications.map((notification) => notification.id);
        for (const id of ids) {
            invoke({ command: 'dismiss', id }).catch((err) =>
                console.error('notifications dismiss failed:', err),
            );
        }
    }

    function clearToasts(): void {
        invoke({ command: 'clear_toasts' }).catch((err) =>
            console.error('clear_toasts failed:', err),
        );
    }

    function badgeLabel(n: number): string {
        return n > 9 ? '9+' : String(n);
    }

    async function openCenter(): Promise<void> {
        // The bar widget is injected with a per-monitor `__quantum_monitor`
        // global (e.g. "DP-1") by the WebView host. Append it as an
        // `@monitor` suffix so the registry pins the notification center to
        // the same monitor as the bar that triggered it.
        const name = monitorView('plugin/notification-center/center');
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

<BarButton ariaLabel="Notifications" onclick={openCenter} bindRef={(el) => (buttonEl = el)}>
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
