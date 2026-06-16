<script lang="ts">
    import { createClient, createNotificationStore, type PendingNotification } from '@quantum/client';

    const client = createClient();

    // The base view name (no monitor suffix). The registry resolves the
    // shown instance from this base name when hiding.
    const VIEW_NAME = 'plugin/notification-center/center';

    let notifications: PendingNotification[] = $state([]);

    $effect(() => {
        const off = createNotificationStore(client).subscribe((list) => {
            notifications = list;
        });
        document.addEventListener('keydown', onKeyDown);
        return () => {
            off?.();
            document.removeEventListener('keydown', onKeyDown);
            client.close();
        };
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
        client.call('view.hide', { name: VIEW_NAME }).catch(() => {});
    }

    function dismiss(id: number): void {
        client
            .call('action.invoke', {
                provider: 'notifications',
                action: {
                    kind: 'custom',
                    data: { kind: 'notifications', payload: { command: 'dismiss', id } },
                },
            })
            .catch(() => {});
    }

    function invokeAction(id: number, actionKey: string): void {
        client
            .call('action.invoke', {
                provider: 'notifications',
                action: {
                    kind: 'custom',
                    data: {
                        kind: 'notifications',
                        payload: { command: 'action', id, action_key: actionKey },
                    },
                },
            })
            .catch(() => {});
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="panel" role="dialog" aria-label="Notifications">
        <header class="panel-header">Notifications</header>

        {#if notifications.length === 0}
            <div class="empty">No notifications</div>
        {:else}
            <div class="list">
                {#each notifications as notification (notification.id)}
                    <div class="card urgency-{notification.urgency}">
                        <div class="icon" aria-hidden="true">
                            {#if notification.icon}
                                <img src={notification.icon} alt="" />
                            {:else}
                                <span class="icon-fallback">{notification.app_name.charAt(0)}</span>
                            {/if}
                        </div>
                        <div class="content">
                            <div class="app-name">{notification.app_name}</div>
                            <div class="summary">{notification.summary}</div>
                            {#if notification.body}
                                <div class="body">{notification.body}</div>
                            {/if}
                            {#if notification.actions.length > 0}
                                <div class="actions">
                                    {#each notification.actions as [key, label] (key)}
                                        <button
                                            class="action"
                                            onclick={() => invokeAction(notification.id, key)}
                                        >
                                            {label}
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        </div>
                        <button
                            class="dismiss"
                            aria-label="Dismiss notification"
                            onclick={() => dismiss(notification.id)}
                        >
                            ×
                        </button>
                    </div>
                {/each}
            </div>
        {/if}
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        display: flex;
        justify-content: flex-end;
        align-items: flex-start;
        padding: 12px;
        background: transparent;
    }
    .panel {
        display: flex;
        flex-direction: column;
        width: 380px;
        max-height: calc(100vh - 24px);
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 12px;
        box-shadow: 0 14px 40px var(--color-shadow, rgba(0, 0, 0, 0.6));
        overflow: hidden;
    }
    .panel-header {
        padding: 14px 16px;
        font-weight: 600;
        font-size: 0.95rem;
        border-bottom: 1px solid var(--color-border, #45475a);
    }
    .empty {
        padding: 28px 16px;
        text-align: center;
        font-size: 0.85rem;
        color: var(--color-fg-muted, #9399b2);
    }
    .list {
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding: 10px;
        overflow-y: auto;
    }
    .card {
        display: flex;
        align-items: flex-start;
        gap: 10px;
        padding: 12px 14px;
        background: var(--color-bg, #1e1e2e);
        border-left: 3px solid var(--color-border, #45475a);
        border-radius: 8px;
    }
    .card.urgency-low {
        border-left-color: var(--color-info, #89b4fa);
    }
    .card.urgency-normal {
        border-left-color: var(--color-accent, #6c5ce7);
    }
    .card.urgency-critical {
        border-left-color: var(--color-danger, #f38ba8);
    }
    .icon {
        flex-shrink: 0;
        width: 28px;
        height: 28px;
        border-radius: 6px;
        background: rgba(255, 255, 255, 0.08);
        display: flex;
        align-items: center;
        justify-content: center;
        overflow: hidden;
    }
    .icon img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }
    .icon-fallback {
        font-size: 0.85rem;
        font-weight: 600;
        text-transform: uppercase;
    }
    .content {
        flex: 1;
        min-width: 0;
    }
    .app-name {
        font-size: 0.65rem;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        color: var(--color-fg-muted, #9399b2);
    }
    .summary {
        font-weight: 600;
        font-size: 0.85rem;
        margin: 2px 0;
        word-break: break-word;
    }
    .body {
        font-size: 0.78rem;
        color: var(--color-fg-muted, #9399b2);
        line-height: 1.35;
        word-break: break-word;
    }
    .actions {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        margin-top: 8px;
    }
    .action {
        padding: 4px 10px;
        font-size: 0.75rem;
        color: var(--color-fg, #cdd6f4);
        background: var(--color-bg-alt, #313244);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 6px;
        cursor: pointer;
    }
    .action:hover {
        background: rgba(255, 255, 255, 0.08);
    }
    .dismiss {
        flex-shrink: 0;
        width: 22px;
        height: 22px;
        line-height: 1;
        font-size: 1.1rem;
        color: var(--color-fg-muted, #9399b2);
        background: transparent;
        border: none;
        border-radius: 4px;
        cursor: pointer;
    }
    .dismiss:hover {
        background: rgba(255, 255, 255, 0.08);
        color: var(--color-fg, #cdd6f4);
    }
</style>
