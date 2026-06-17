<script lang="ts">
    import { createTimerStore, type Client } from '@quantum/client';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';
    import { onClick } from './interaction';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let count = $state(0);
    let menuOpen = $state(false);
    let root: HTMLElement | undefined = $state(undefined);
    let buttonEl: HTMLButtonElement | undefined = $state(undefined);

    $effect(() => {
        const off = createTimerStore(client).subscribe((data) => {
            count = data.timers.filter((t) => t.status === 'active').length;
        });
        return () => off?.();
    });

    // Right-click opens the quick-actions popover.
    //
    // WebKit caveat: `onClick(..., 'right')` (in interaction.ts) calls
    // `event.preventDefault()` on the `contextmenu` event, which suppresses
    // WebKitGTK's native context menu for the button. No host-side WebKit
    // context-menu policy change is required because the popover itself is
    // plain DOM rendered inside the bar view.
    $effect(() => {
        const node = buttonEl;
        if (!node) return;
        return onClick(node, () => {
            menuOpen = true;
        }, 'right');
    });

    // While the popover is open, close it on Escape or on any click that
    // lands outside the indicator's root element.
    $effect(() => {
        if (!menuOpen) return;
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') menuOpen = false;
        };
        const onDocumentClick = (event: MouseEvent) => {
            if (root && !root.contains(event.target as Node)) menuOpen = false;
        };
        window.addEventListener('keydown', onKeyDown);
        document.addEventListener('click', onDocumentClick);
        return () => {
            window.removeEventListener('keydown', onKeyDown);
            document.removeEventListener('click', onDocumentClick);
        };
    });

    function badgeLabel(n: number): string {
        return n > 9 ? '9+' : String(n);
    }

    async function openCreate(): Promise<void> {
        // The bar widget is injected with a per-monitor `__quantum_monitor`
        // global (e.g. "DP-1") by the WebView host. Append it as an
        // `@monitor` suffix so the registry pins the timer-create overlay to
        // the same monitor as the bar that triggered it.
        const monitor = window.__quantum_monitor;
        const name = monitor
            ? `plugin/timer-create/timer-create@${monitor}`
            : 'plugin/timer-create/timer-create';
        try {
            await client.call('view.toggle', { name });
        } catch (err) {
            console.error(`view.toggle ${name} failed:`, err);
        }
    }

    async function dismissAll(): Promise<void> {
        menuOpen = false;
        try {
            await client.call('timer.dismiss_all', {});
        } catch (err) {
            console.error('timer.dismiss_all failed:', err);
        }
    }

    async function openFromMenu(): Promise<void> {
        menuOpen = false;
        await openCreate();
    }
</script>

<div class="timer-root" bind:this={root}>
    <BarButton ariaLabel="Timers" onclick={openCreate} bindRef={(el) => (buttonEl = el)}>
        <span class="timer-icon">
            <Icon name="timer" size={18} />
            {#if count > 0}
                <span class="timer-badge">{badgeLabel(count)}</span>
            {/if}
        </span>
    </BarButton>
    {#if menuOpen}
        <div class="timer-menu" role="menu">
            <button type="button" role="menuitem" onclick={dismissAll}>Dismiss all</button>
            <button type="button" role="menuitem" onclick={openFromMenu}>Open timers</button>
        </div>
    {/if}
</div>

<style>
    .timer-root {
        position: relative;
        display: inline-flex;
    }
    .timer-icon {
        position: relative;
        display: inline-flex;
        align-items: center;
        justify-content: center;
    }
    .timer-badge {
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
    .timer-menu {
        position: absolute;
        top: calc(100% + 6px);
        right: 0;
        z-index: 10;
        display: flex;
        flex-direction: column;
        min-width: 140px;
        padding: 4px;
        border-radius: 10px;
        background: var(--color-surface, hsla(230, 14%, 22%, 0.98));
        border: 1px solid rgba(255, 255, 255, 0.08);
        box-shadow: 0 6px 18px rgba(0, 0, 0, 0.35);
    }
    .timer-menu button {
        display: block;
        width: 100%;
        text-align: left;
        background: transparent;
        border: none;
        border-radius: 6px;
        padding: 6px 10px;
        color: var(--color-fg-alt, #a6adc8);
        font-family: inherit;
        font-size: inherit;
        line-height: 1.2;
        cursor: pointer;
    }
    .timer-menu button:hover,
    .timer-menu button:focus-visible {
        background: var(--color-surface-hover, hsla(230, 14%, 42%, 1));
        color: var(--color-fg, #cdd6f4);
        outline: none;
    }
</style>
