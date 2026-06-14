<script lang="ts">
    import { createClient } from '@quantum/client';
    import { WIFI_PROVIDER, WIFI_CHANNEL } from './lib/channels';
    import type { WifiState } from './lib/types';

    const client = createClient();

    let state: WifiState = $state({
        available: false,
        radio_enabled: false,
        scanning: false,
        active: null,
        networks: [],
        saved: [],
    });

    function send(payload: Record<string, unknown>): Promise<unknown> {
        return client.call('action.invoke', {
            provider: WIFI_PROVIDER,
            action: { kind: 'custom', data: { kind: 'wifi', payload } },
        });
    }

    $effect(() => {
        client
            .call('provider.query', { id: WIFI_PROVIDER })
            .then((r: unknown) => { if (r) state = r as WifiState; })
            .catch(() => {});
        const off = client.subscribe(WIFI_CHANNEL, (p: unknown) => { state = p as WifiState; });
        send({ command: 'open_session' }).catch(() => {});
        return () => {
            send({ command: 'close_session' }).catch(() => {});
            off?.();
            client.close();
        };
    });

    $effect(() => {
        document.addEventListener('keydown', onKeyDown);
        return () => document.removeEventListener('keydown', onKeyDown);
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
        client.call('view.hide', { name: 'widgets/wifi-menu' }).catch(() => {});
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="Wi-Fi">
        <h2>Wi-Fi</h2>
        <p>{state.networks.length} networks{state.scanning ? ' (scanning)' : ''}</p>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        background: var(--color-overlay-backdrop, rgba(0, 0, 0, 0.5));
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .card {
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border-radius: 12px;
        padding: 18px 20px;
        width: 420px;
        border: 1px solid var(--color-border, #45475a);
    }
</style>
