<script lang="ts">
    import { createClient, openContextMenu, type MenuItem } from '@quantum/client';
    import { WIFI_PROVIDER, WIFI_CHANNEL } from './lib/channels';
    import type { WifiState, WifiNetwork, SavedNetwork, Ipv4Method } from './lib/types';
    import NetworkRow from './lib/NetworkRow.svelte';
    import PasswordForm from './lib/PasswordForm.svelte';
    import HiddenForm from './lib/HiddenForm.svelte';
    import SavedRow from './lib/SavedRow.svelte';
    import DetailsPanel from './lib/DetailsPanel.svelte';
    import AdvancedPanel from './lib/AdvancedPanel.svelte';

    const client = createClient();

    let state: WifiState = $state({
        available: false,
        radio_enabled: false,
        scanning: false,
        active: null,
        networks: [],
        saved: [],
    });

    /** Which top-level panel is showing. */
    type Panel = 'available' | 'saved' | 'details' | 'advanced';
    let panel: Panel = $state('available');

    /** Bssid of the row whose inline password form is expanded. */
    let passwordFor: string | null = $state(null);
    /** Whether the footer hidden-network form is open. */
    let hiddenOpen = $state(false);
    /** Saved network whose advanced panel is showing. */
    let advancedNetwork: SavedNetwork | null = $state(null);

    /**
     * Per-network connect status keyed by bssid (fallback ssid). Driven
     * by the connect flow: set to 'connecting' on click, cleared on
     * success (the stream reflects the new active connection), or set to
     * an error variant on rejection.
     */
    type ConnectStatus = 'connecting' | 'error' | 'incorrect_password';
    let status: Record<string, ConnectStatus> = $state({});

    /**
     * The single command envelope. Every action goes through here so the
     * nested custom shape is defined in exactly one place.
     */
    function send(payload: Record<string, unknown>): Promise<unknown> {
        return client.call('action.invoke', {
            provider: WIFI_PROVIDER,
            action: { kind: 'custom', data: { kind: 'wifi', payload } },
        });
    }

    /**
     * Fire a command whose failure has no inline UI surface and log any
     * rejection, matching the house pattern in power-menu rather than
     * swallowing errors silently. Commands that drive inline status
     * (connect) handle their own rejections separately.
     */
    function sendFireAndForget(payload: Record<string, unknown>): void {
        send(payload).catch((error) => {
            console.error(`wifi ${String(payload.command)} failed:`, error);
        });
    }

    $effect(() => {
        client
            .call('provider.query', { id: WIFI_PROVIDER })
            .then((r: unknown) => {
                if (r) state = r as WifiState;
            })
            .catch(() => {});
        const off = client.subscribe(WIFI_CHANNEL, (p: unknown) => {
            state = p as WifiState;
        });
        sendFireAndForget({ command: 'open_session' });
        return () => {
            sendFireAndForget({ command: 'close_session' });
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

    /**
     * Inspect a rejected client.call value for the daemon's failure
     * reason. The reason string rides in the JSON-RPC error message or
     * data; we stringify and look for the known substring.
     */
    function classifyError(reason: unknown): ConnectStatus {
        const text = JSON.stringify(reason ?? '').toLowerCase();
        if (text.includes('incorrect_password')) return 'incorrect_password';
        return 'error';
    }

    function rowKey(network: WifiNetwork): string {
        return network.bssid || network.ssid;
    }

    /**
     * Click on an available-network row. Open or saved networks connect
     * straight away with no password; a secured, unsaved network expands
     * the inline password form instead.
     */
    function onNetworkSelect(network: WifiNetwork): void {
        const needsPassword =
            network.security !== 'open' && !network.saved && !network.active;
        if (needsPassword) {
            passwordFor = rowKey(network);
            return;
        }
        void connect(network, null);
    }

    async function connect(network: WifiNetwork, password: string | null): Promise<void> {
        const key = rowKey(network);
        status = { ...status, [key]: 'connecting' };
        const payload: Record<string, unknown> = { command: 'connect', ssid: network.ssid };
        if (password !== null) payload.password = password;
        try {
            await send(payload);
            const next = { ...status };
            delete next[key];
            status = next;
            passwordFor = null;
        } catch (reason) {
            status = { ...status, [key]: classifyError(reason) };
        }
    }

    function onPasswordSubmit(network: WifiNetwork, password: string): void {
        void connect(network, password);
    }

    function onPasswordCancel(): void {
        passwordFor = null;
    }

    function toggleRadio(): void {
        sendFireAndForget({ command: 'set_radio', enabled: !state.radio_enabled });
    }

    function rescan(): void {
        sendFireAndForget({ command: 'rescan' });
    }

    function onForget(id: string): void {
        sendFireAndForget({ command: 'forget', id });
        if (advancedNetwork && advancedNetwork.id === id) {
            advancedNetwork = null;
            panel = 'saved';
        }
    }

    function openAdvanced(network: SavedNetwork): void {
        advancedNetwork = network;
        panel = 'advanced';
    }

    /** Open the advanced panel from an available row's cog (saved only). */
    function openAdvancedForNetwork(network: WifiNetwork): void {
        const saved = state.saved.find((entry) => entry.ssid === network.ssid);
        if (saved) openAdvanced(saved);
    }

    function openDetails(): void {
        if (state.active) panel = 'details';
    }

    function onHiddenSubmit(ssid: string, password: string): void {
        const payload: Record<string, unknown> = { command: 'connect_hidden', ssid };
        if (password.trim() !== '') payload.password = password;
        sendFireAndForget(payload);
        hiddenOpen = false;
    }

    function onSetAutoconnect(id: string, enabled: boolean): void {
        sendFireAndForget({ command: 'set_autoconnect', id, enabled });
    }

    function onSetMetered(id: string, metered: boolean): void {
        sendFireAndForget({ command: 'set_metered', id, metered });
    }

    function onSetIpv4(
        id: string,
        method: Ipv4Method,
        address: string | null,
        gateway: string | null,
        prefix: number | null,
    ): void {
        const payload: Record<string, unknown> = { command: 'set_ipv4', id, method };
        if (address !== null) payload.address = address;
        if (gateway !== null) payload.gateway = gateway;
        if (prefix !== null) payload.prefix = prefix;
        sendFireAndForget(payload);
    }

    function onSetDns(id: string, servers: string[]): void {
        sendFireAndForget({ command: 'set_dns', id, servers });
    }

    function onFetchDetails(ssid: string): void {
        sendFireAndForget({ command: 'fetch_details', ssid });
    }

    function selectTab(next: 'available' | 'saved'): void {
        panel = next;
        passwordFor = null;
        hiddenOpen = false;
    }

    // Right-click an available network: connect, autoconnect toggle, forget.
    function networkMenu(event: MouseEvent, network: WifiNetwork): void {
        const saved = state.saved.find((entry) => entry.ssid === network.ssid);
        const items: MenuItem[] = [];
        if (!network.active) {
            items.push({ label: 'Connect', onSelect: () => onNetworkSelect(network) });
        }
        if (saved) {
            items.push({
                label: saved.autoconnect ? 'Disable autoconnect' : 'Enable autoconnect',
                onSelect: () => onSetAutoconnect(saved.id, !saved.autoconnect),
            });
            items.push({ separator: true });
            items.push({
                label: 'Forget network',
                danger: true,
                onSelect: () => onForget(saved.id),
            });
        }
        if (items.length === 0) return;
        openContextMenu(event, items);
    }

    // Right-click the header: rescan and toggle the radio.
    function headerMenu(event: MouseEvent): void {
        openContextMenu(event, [
            { label: 'Rescan', disabled: !state.radio_enabled, onSelect: rescan },
            {
                label: state.radio_enabled ? 'Turn Wi-Fi off' : 'Turn Wi-Fi on',
                onSelect: toggleRadio,
            },
        ]);
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="Wi-Fi">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="card-head" oncontextmenu={headerMenu}>
            <span class="title">Wi-Fi</span>
            <span class="toggle-pill">
                {state.radio_enabled ? 'On' : 'Off'}
                <button
                    type="button"
                    class="switch"
                    class:on={state.radio_enabled}
                    data-action="radio"
                    aria-label="Toggle Wi-Fi radio"
                    aria-pressed={state.radio_enabled}
                    onclick={toggleRadio}
                ></button>
            </span>
        </div>

        {#if !state.radio_enabled}
            <div class="empty radio-off">
                Wi-Fi is turned off.
                <button type="button" class="btn" data-action="radio-on" onclick={toggleRadio}>
                    Turn on WiFi
                </button>
            </div>
        {:else if panel === 'details' && state.active}
            <DetailsPanel
                active={state.active}
                onBack={() => (panel = 'available')}
                onFetch={onFetchDetails}
            />
        {:else if panel === 'advanced' && advancedNetwork}
            <AdvancedPanel
                network={advancedNetwork}
                onBack={() => (panel = 'saved')}
                {onSetAutoconnect}
                {onSetMetered}
                {onSetIpv4}
                {onSetDns}
                {onForget}
            />
        {:else}
            <div class="tabs">
                <button
                    type="button"
                    class:active={panel === 'available'}
                    data-tab="available"
                    onclick={() => selectTab('available')}
                >
                    Available
                </button>
                <button
                    type="button"
                    class:active={panel === 'saved'}
                    data-tab="saved"
                    onclick={() => selectTab('saved')}
                >
                    Saved
                </button>
            </div>

            {#if panel === 'available'}
                <div class="scanbar">
                    {#if state.scanning}
                        <span class="spinner"></span> Scanning...
                    {:else}
                        Last scan just now
                    {/if}
                    <button type="button" class="refresh-btn" data-action="rescan" onclick={rescan}>
                        Refresh
                    </button>
                </div>
                {#if state.active}
                    <div class="details-link-row">
                        <button
                            type="button"
                            class="foot-link"
                            data-action="details"
                            onclick={openDetails}
                        >
                            Connection details
                        </button>
                    </div>
                {/if}
                <div class="scroll">
                    {#if state.networks.length === 0}
                        <div class="empty">Searching for networks...</div>
                    {/if}
                    {#each state.networks as network (rowKey(network))}
                        <!-- svelte-ignore a11y_no_static_element_interactions -->
                        <div
                            class="row-context"
                            oncontextmenu={(event) => networkMenu(event, network)}
                        >
                            <NetworkRow
                                {network}
                                status={status[rowKey(network)] ?? null}
                                onSelect={() => onNetworkSelect(network)}
                                onSettings={network.saved
                                    ? () => openAdvancedForNetwork(network)
                                    : null}
                            />
                        </div>
                        {#if passwordFor === rowKey(network)}
                            <PasswordForm
                                ssid={network.ssid}
                                error={(() => {
                                    const current = status[rowKey(network)];
                                    return current === 'incorrect_password'
                                        ? 'incorrect_password'
                                        : current === 'error'
                                          ? 'error'
                                          : null;
                                })()}
                                onSubmit={(password) => onPasswordSubmit(network, password)}
                                onCancel={onPasswordCancel}
                            />
                        {/if}
                    {/each}
                    {#if hiddenOpen}
                        <HiddenForm
                            onSubmit={onHiddenSubmit}
                            onCancel={() => (hiddenOpen = false)}
                        />
                    {/if}
                </div>
            {:else}
                <div class="scroll">
                    {#if state.saved.length === 0}
                        <div class="empty">No saved networks.</div>
                    {/if}
                    {#each state.saved as network (network.id)}
                        <SavedRow
                            {network}
                            onSettings={() => openAdvanced(network)}
                            onForget={() => onForget(network.id)}
                        />
                    {/each}
                </div>
            {/if}

            <div class="card-foot">
                <button
                    type="button"
                    class="foot-link"
                    data-action="hidden"
                    onclick={() => (hiddenOpen = !hiddenOpen)}
                >
                    + Connect to hidden network
                </button>
            </div>
        {/if}
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        background: var(--color-overlay-backdrop, rgba(0, 0, 0, 0.5));
        backdrop-filter: blur(4px);
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .card {
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border-radius: 12px;
        width: min(420px, 92vw);
        max-height: 80vh;
        box-sizing: border-box;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        box-shadow: 0 14px 40px var(--color-shadow, rgba(0, 0, 0, 0.6));
        border: 1px solid var(--color-border, #45475a);
    }
    .card-head {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 14px 16px;
        border-bottom: 1px solid var(--color-border, #45475a);
    }
    .card-head .title {
        font-size: 14px;
        font-weight: 600;
        flex: 1;
    }
    .toggle-pill {
        display: inline-flex;
        align-items: center;
        gap: 7px;
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
    }
    .switch {
        width: 34px;
        height: 19px;
        border-radius: 10px;
        background: var(--color-border, #45475a);
        position: relative;
        cursor: pointer;
        transition: background 0.15s;
        border: none;
        padding: 0;
    }
    .switch.on {
        background: var(--color-accent, #89b4fa);
    }
    .switch::after {
        content: '';
        position: absolute;
        top: 2px;
        left: 2px;
        width: 15px;
        height: 15px;
        border-radius: 50%;
        background: #fff;
        transition: transform 0.15s;
    }
    .switch.on::after {
        transform: translateX(15px);
    }
    .tabs {
        display: flex;
        padding: 0 8px;
        border-bottom: 1px solid var(--color-border, #45475a);
    }
    .tabs button {
        background: none;
        border: none;
        color: var(--color-fg-alt, #a6adc8);
        padding: 10px 12px;
        font-size: 12px;
        cursor: pointer;
        font-family: inherit;
        border-bottom: 2px solid transparent;
        margin-bottom: -1px;
    }
    .tabs button.active {
        color: var(--color-accent, #89b4fa);
        border-bottom-color: var(--color-accent, #89b4fa);
    }
    .scroll {
        overflow-y: auto;
        padding: 6px;
        flex: 1;
    }
    .scanbar {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 6px 10px;
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
    }
    .refresh-btn {
        margin-left: auto;
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 6px;
        padding: 3px 8px;
        font-size: 11px;
        cursor: pointer;
        font-family: inherit;
    }
    .refresh-btn:hover {
        border-color: var(--color-accent, #89b4fa);
        color: var(--color-accent, #89b4fa);
    }
    .spinner {
        width: 12px;
        height: 12px;
        border-radius: 50%;
        border: 2px solid var(--color-border, #45475a);
        border-top-color: var(--color-accent, #89b4fa);
        animation: spin 0.7s linear infinite;
    }
    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }
    .details-link-row {
        padding: 0 12px 4px;
    }
    .empty {
        text-align: center;
        color: var(--color-fg-alt, #a6adc8);
        font-size: 12px;
        padding: 40px 20px;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 16px;
    }
    .empty.radio-off {
        padding: 60px 20px;
    }
    .btn {
        background: var(--color-accent, #89b4fa);
        color: var(--color-bg, #1e1e2e);
        border: none;
        border-radius: 7px;
        padding: 7px 14px;
        font-size: 12px;
        font-weight: 600;
        cursor: pointer;
        font-family: inherit;
    }
    .card-foot {
        padding: 10px 14px;
        border-top: 1px solid var(--color-border, #45475a);
        display: flex;
        gap: 8px;
        justify-content: space-between;
        align-items: center;
    }
    .foot-link {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
        cursor: pointer;
        background: none;
        border: none;
        padding: 0;
        font-family: inherit;
    }
    .foot-link:hover {
        color: var(--color-accent, #89b4fa);
    }
</style>
