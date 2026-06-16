<script lang="ts">
    import { createClient, createNotificationStore, type PendingNotification } from '@quantum/client';

    const client = createClient();

    // The base view name (no monitor suffix). The registry resolves the
    // shown instance from this base name when hiding.
    const VIEW_NAME = 'plugin/notification-center/toast';

    // Fallback auto-dismiss when a notification carries timeout_ms === 0
    // (the server's "no explicit expiry" sentinel). Mirrors plugin.toml's
    // default_timeout.
    const DEFAULT_TIMEOUT_MS = 5000;

    // Currently-visible toasts, newest first (rendered top to bottom).
    let visible: PendingNotification[] = $state([]);

    // Active auto-dismiss timers keyed by notification id. Lets us clear a
    // toast's timer on click/unmount without leaking handles.
    const timers = new Map<number, ReturnType<typeof setTimeout>>();

    // Ids we have already surfaced as a toast. Because every snapshot carries
    // the full pending set, a notification that has already timed out visually
    // still appears in later snapshots; tracking seen ids stops it re-popping.
    const seen = new Set<number>();

    $effect(() => {
        const off = createNotificationStore(client).subscribe((list) => {
            for (const notification of list) {
                if (seen.has(notification.id)) continue;
                seen.add(notification.id);
                // Newest on top.
                visible = [notification, ...visible];
                const delay = notification.timeout_ms > 0 ? notification.timeout_ms : DEFAULT_TIMEOUT_MS;
                const id = notification.id;
                timers.set(
                    id,
                    setTimeout(() => {
                        timers.delete(id);
                        removeToast(id);
                    }, delay),
                );
            }
        });
        return () => {
            off?.();
            for (const handle of timers.values()) clearTimeout(handle);
            timers.clear();
            client.close();
        };
    });

    function removeToast(id: number): void {
        const timer = timers.get(id);
        if (timer !== undefined) {
            clearTimeout(timer);
            timers.delete(id);
        }
        visible = visible.filter((notification) => notification.id !== id);
        // When nothing is left to show, unmap the overlay surface so it stops
        // covering the desktop corner.
        if (visible.length === 0) {
            client.call('view.hide', { name: VIEW_NAME }).catch(() => {});
        }
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
        removeToast(id);
    }
</script>

<div class="stack">
    {#each visible as notification (notification.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div class="toast urgency-{notification.urgency}" onclick={() => dismiss(notification.id)}>
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
            </div>
        </div>
    {/each}
</div>

<style>
    :global(html),
    :global(body) {
        margin: 0;
        padding: 0;
        background: transparent;
    }
    .stack {
        position: fixed;
        top: 12px;
        right: 12px;
        width: 380px;
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .toast {
        display: flex;
        align-items: flex-start;
        gap: 10px;
        padding: 12px 14px;
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border-left: 3px solid var(--color-border, #45475a);
        border-radius: 8px;
        box-shadow: 0 14px 40px var(--color-shadow, rgba(0, 0, 0, 0.6));
        cursor: pointer;
    }
    .toast.urgency-low {
        border-left-color: var(--color-info, #89b4fa);
    }
    .toast.urgency-normal {
        border-left-color: var(--color-accent, #6c5ce7);
    }
    .toast.urgency-critical {
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
</style>
