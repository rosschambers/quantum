<script lang="ts">
    import { createClient, createNotificationStore, type PendingNotification } from '@quantum/client';

    const client = createClient();

    // The base view name (no monitor suffix). The registry resolves the
    // shown instance from this base name when hiding.
    const VIEW_NAME = 'plugin/notification-center/toast';

    // Toast on-screen lifetime, in milliseconds. A toast is the TRANSIENT
    // popup; the notification itself persists in the notification center
    // regardless, so every toast auto-dismisses. Apps that request a very short
    // expiry would blink past unread, so positive timeouts are floored to MIN;
    // very long (or "never expire", 0) timeouts are capped to MAX/DEFAULT so the
    // popup never sits on screen indefinitely.
    const DEFAULT_VISIBLE_MS = 5000;
    const MIN_VISIBLE_MS = 3000;
    const MAX_VISIBLE_MS = 10000;

    // How long the stack may stay empty before the overlay surface is unmapped.
    // The daemon shows this window on every `created` event, so an empty window
    // must never stay mapped (it captures pointer input over the screen corner).
    // This must comfortably outlast the daemon's show->subscribe delivery so a
    // real notification renders before the guard fires, yet be short enough that
    // a stuck-empty window clears quickly.
    const HIDE_DEBOUNCE_MS = 600;

    // Currently-visible toasts, newest first (rendered top to bottom).
    let visible: PendingNotification[] = $state([]);

    // The on-screen lifetime a toast was armed with. Drives the progress bar's
    // CSS animation duration.
    function visibleDurationMs(notification: PendingNotification): number {
        if (notification.timeout_ms > 0) {
            return Math.min(Math.max(notification.timeout_ms, MIN_VISIBLE_MS), MAX_VISIBLE_MS);
        }
        return DEFAULT_VISIBLE_MS;
    }

    // Active auto-dismiss timers keyed by notification id. Each tracks its
    // handle plus enough state to pause on hover and resume on leave.
    interface ToastTimer {
        handle: ReturnType<typeof setTimeout>;
        startedAt: number;
        remaining: number;
    }
    const timers = new Map<number, ToastTimer>();

    // Ids we have already surfaced as a toast. Because every snapshot carries
    // the full pending set, a notification that has already timed out visually
    // still appears in later snapshots; tracking seen ids stops it re-popping.
    const seen = new Set<number>();

    function armTimer(id: number, ms: number): void {
        const handle = setTimeout(() => {
            timers.delete(id);
            removeToast(id);
        }, ms);
        timers.set(id, { handle, startedAt: Date.now(), remaining: ms });
    }

    // Pause a toast's auto-dismiss while the pointer is over it, banking the
    // time left. The progress bar pauses in CSS on the same `:hover`.
    function pauseTimer(id: number): void {
        const timer = timers.get(id);
        if (timer === undefined) return;
        clearTimeout(timer.handle);
        timer.remaining = Math.max(0, timer.remaining - (Date.now() - timer.startedAt));
    }

    // Resume a paused toast with the banked remaining time.
    function resumeTimer(id: number): void {
        const timer = timers.get(id);
        if (timer === undefined) return;
        timer.startedAt = Date.now();
        timer.handle = setTimeout(() => {
            timers.delete(id);
            removeToast(id);
        }, timer.remaining);
    }

    // Clear every on-screen toast at once (e.g. when the user opens the
    // notification center). The notifications themselves are untouched in the
    // store; only the transient popups are dismissed and the surface unmapped.
    function clearAllToasts(): void {
        for (const timer of timers.values()) clearTimeout(timer.handle);
        timers.clear();
        visible = [];
        client.call('view.hide', { name: VIEW_NAME }).catch(() => {});
    }

    $effect(() => {
        const off = createNotificationStore(client).subscribe(
            (list) => {
                for (const notification of list) {
                    if (seen.has(notification.id)) continue;
                    seen.add(notification.id);
                    // Newest on top.
                    visible = [notification, ...visible];
                    armTimer(notification.id, visibleDurationMs(notification));
                }
            },
            (change) => {
                if (change?.type === 'toasts_cleared') clearAllToasts();
            },
        );
        return () => {
            off?.();
            for (const timer of timers.values()) clearTimeout(timer.handle);
            timers.clear();
            client.close();
        };
    });

    // Safety net: whenever the stack is empty, schedule an unmap of the overlay
    // surface. If a card arrives before the debounce elapses (the show->subscribe
    // race), the effect re-runs and the teardown cancels the pending hide, so a
    // real notification is never hidden before it renders.
    $effect(() => {
        if (visible.length !== 0) return;
        const handle = setTimeout(() => {
            if (visible.length === 0) {
                client.call('view.hide', { name: VIEW_NAME }).catch(() => {});
            }
        }, HIDE_DEBOUNCE_MS);
        return () => clearTimeout(handle);
    });

    function removeToast(id: number): void {
        const timer = timers.get(id);
        if (timer !== undefined) {
            clearTimeout(timer.handle);
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
        <div
            class="toast urgency-{notification.urgency}"
            onclick={() => dismiss(notification.id)}
            onmouseenter={() => pauseTimer(notification.id)}
            onmouseleave={() => resumeTimer(notification.id)}
        >
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
            <!-- Auto-dismiss countdown. The CSS animation runs for the toast's
                 lifetime and pauses on :hover, in lockstep with the JS timer
                 paused by onmouseenter/onmouseleave. -->
            <div
                class="progress"
                style="animation-duration: {visibleDurationMs(notification)}ms"
            ></div>
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
        width: min(380px, 92vw);
        max-width: calc(100vw - 24px);
        box-sizing: border-box;
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .toast {
        position: relative;
        overflow: hidden;
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
    /* Auto-dismiss countdown bar across the bottom edge. It shrinks from full
       to empty over the toast's lifetime, and pauses while the toast is
       hovered (matching the JS dismiss timer paused on the same hover). */
    .progress {
        position: absolute;
        left: 0;
        bottom: 0;
        height: 3px;
        width: 100%;
        transform-origin: left;
        background: var(--color-accent, #6c5ce7);
        animation-name: toast-progress;
        animation-timing-function: linear;
        animation-fill-mode: forwards;
    }
    .toast:hover .progress {
        animation-play-state: paused;
    }
    .toast.urgency-low .progress {
        background: var(--color-info, #89b4fa);
    }
    .toast.urgency-normal .progress {
        background: var(--color-accent, #6c5ce7);
    }
    .toast.urgency-critical .progress {
        background: var(--color-danger, #f38ba8);
    }
    @keyframes toast-progress {
        from {
            transform: scaleX(1);
        }
        to {
            transform: scaleX(0);
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .progress {
            animation: none;
        }
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
