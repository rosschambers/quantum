<script lang="ts">
    import { createTimerStore, openContextMenu, type Client } from '@quantum/client';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let count = $state(0);
    let buttonEl: HTMLButtonElement | undefined = $state(undefined);

    $effect(() => {
        const off = createTimerStore(client).subscribe((data) => {
            count = data.timers.filter((t) => t.status === 'active').length;
        });
        return () => off?.();
    });

    // The bar is a thin strip (Layer::Top), so a downward menu is clipped
    // until the surface grows. ensureSpace/restoreHeight grow this monitor's
    // bar surface via view.set_height for the lifetime of the menu.
    const BAR_HEIGHT = 32;

    function barViewName(): string {
        const monitor = (window as unknown as { __quantum_monitor?: string })
            .__quantum_monitor;
        return monitor ? `plugin/bar/bar@${monitor}` : 'plugin/bar/bar';
    }

    function ensureSpace(neededPixels: number): Promise<void> {
        return client
            .call('view.set_height', {
                name: barViewName(),
                height: Math.ceil(neededPixels) + 8,
            })
            .then(() => undefined);
    }

    function restoreHeight(): void {
        client
            .call('view.set_height', { name: barViewName(), height: BAR_HEIGHT })
            .catch(() => {});
    }

    // Right-click opens the quick-actions menu via the shared context-menu
    // runtime. The runtime calls preventDefault, so WebKitGTK's native menu is
    // suppressed without any host-side policy change.
    $effect(() => {
        const node = buttonEl;
        if (!node) return;
        const listener = (event: MouseEvent): void => {
            openContextMenu(
                event,
                [
                    { label: 'Open timers', onSelect: openCreate },
                    { label: 'Dismiss all', onSelect: dismissAll },
                ],
                { ensureSpace, onClose: restoreHeight },
            );
        };
        node.addEventListener('contextmenu', listener);
        return () => node.removeEventListener('contextmenu', listener);
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
        try {
            await client.call('timer.dismiss_all', {});
        } catch (err) {
            console.error('timer.dismiss_all failed:', err);
        }
    }
</script>

<div class="timer-root">
    <BarButton ariaLabel="Timers" onclick={openCreate} bindRef={(el) => (buttonEl = el)}>
        <span class="timer-icon">
            <Icon name="timer" size={18} />
            {#if count > 0}
                <span class="timer-badge">{badgeLabel(count)}</span>
            {/if}
        </span>
    </BarButton>
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
</style>
