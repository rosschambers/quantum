<script lang="ts">
    import type { SavedNetwork, Ipv4Method } from './types';

    /**
     * Per-network advanced settings. Exposes the five backend controls:
     * auto-connect, metered, IPv4 method (auto/manual with address,
     * gateway, prefix), custom DNS servers, and forget.
     *
     * SavedNetwork carries no current metered/IPv4/DNS values, so those
     * fields start blank and must NOT be written back implicitly. The
     * auto-connect and metered toggles fire immediately because a toggle
     * is a deliberate user action with a known target value. The IPv4
     * and DNS sections only send their command when the user clicks the
     * explicit Apply button, so merely opening and closing the panel
     * never clobbers a network's real configuration with blank defaults.
     */
    interface Props {
        network: SavedNetwork;
        onBack: () => void;
        onSetAutoconnect: (id: string, enabled: boolean) => void;
        onSetMetered: (id: string, metered: boolean) => void;
        onSetIpv4: (
            id: string,
            method: Ipv4Method,
            address: string | null,
            gateway: string | null,
            prefix: number | null,
        ) => void;
        onSetDns: (id: string, servers: string[]) => void;
        onForget: (id: string) => void;
    }

    const {
        network,
        onBack,
        onSetAutoconnect,
        onSetMetered,
        onSetIpv4,
        onSetDns,
        onForget,
    }: Props = $props();

    // Seed the toggle from the network's current state. The panel is
    // remounted whenever a different saved network is opened, so the
    // initial-value capture is the intended behaviour.
    // svelte-ignore state_referenced_locally
    let autoconnect = $state(network.autoconnect);
    let metered = $state(false);
    let ipv4Method: Ipv4Method = $state('auto');
    let address = $state('');
    let gateway = $state('');
    let prefix = $state('');
    let dns = $state('');

    function toggleAutoconnect(): void {
        autoconnect = !autoconnect;
        onSetAutoconnect(network.id, autoconnect);
    }

    function toggleMetered(): void {
        metered = !metered;
        onSetMetered(network.id, metered);
    }

    function selectMethod(method: Ipv4Method): void {
        ipv4Method = method;
    }

    /**
     * Explicit IPv4 apply. For automatic mode this sends a clean
     * auto config; for manual mode it sends whatever the user typed.
     * Only ever runs on a deliberate button click.
     */
    function applyIpv4(): void {
        if (ipv4Method === 'auto') {
            onSetIpv4(network.id, 'auto', null, null, null);
            return;
        }
        const parsedPrefix = prefix.trim() === '' ? null : Number(prefix);
        onSetIpv4(
            network.id,
            'manual',
            address.trim() === '' ? null : address.trim(),
            gateway.trim() === '' ? null : gateway.trim(),
            parsedPrefix !== null && Number.isNaN(parsedPrefix) ? null : parsedPrefix,
        );
    }

    /** Explicit DNS apply. Only ever runs on a deliberate button click. */
    function applyDns(): void {
        const servers = dns
            .split(',')
            .map((entry) => entry.trim())
            .filter((entry) => entry !== '');
        onSetDns(network.id, servers);
    }
</script>

<div class="detail-head">
    <button type="button" class="back" data-action="back" aria-label="Back" onclick={onBack}>
        &#8592;
    </button>
    <span class="title">{network.ssid} — settings</span>
</div>
<div class="scroll">
    <div class="adv-toggle">
        <span>Connect automatically</span>
        <button
            type="button"
            class="switch"
            class:on={autoconnect}
            data-action="autoconnect"
            aria-label="Toggle auto-connect"
            aria-pressed={autoconnect}
            onclick={toggleAutoconnect}
        ></button>
    </div>
    <div class="adv-toggle">
        <span>Metered connection</span>
        <button
            type="button"
            class="switch"
            class:on={metered}
            data-action="metered"
            aria-label="Toggle metered"
            aria-pressed={metered}
            onclick={toggleMetered}
        ></button>
    </div>

    <div class="adv-section-title">IPv4</div>
    <div class="form">
        <div class="field">
            <label for="ipv4-method">Method</label>
            <div class="segmini" id="ipv4-method">
                <button
                    type="button"
                    class:active={ipv4Method === 'auto'}
                    data-action="ipv4-auto"
                    onclick={() => selectMethod('auto')}
                >
                    Automatic (DHCP)
                </button>
                <button
                    type="button"
                    class:active={ipv4Method === 'manual'}
                    data-action="ipv4-manual"
                    onclick={() => selectMethod('manual')}
                >
                    Manual
                </button>
            </div>
        </div>
        <div class="field">
            <label for="ipv4-address">IP address</label>
            <input
                id="ipv4-address"
                type="text"
                placeholder="192.168.1.50"
                bind:value={address}
                disabled={ipv4Method === 'auto'}
            />
        </div>
        <div class="field">
            <label for="ipv4-prefix">Prefix</label>
            <input
                id="ipv4-prefix"
                type="text"
                placeholder="24"
                bind:value={prefix}
                disabled={ipv4Method === 'auto'}
            />
        </div>
        <div class="field">
            <label for="ipv4-gateway">Gateway</label>
            <input
                id="ipv4-gateway"
                type="text"
                placeholder="192.168.1.1"
                bind:value={gateway}
                disabled={ipv4Method === 'auto'}
            />
        </div>
        <button type="button" class="btn" data-action="apply-ipv4" onclick={applyIpv4}>
            Apply IPv4
        </button>
    </div>

    <div class="adv-section-title">DNS</div>
    <div class="form">
        <div class="field">
            <label for="dns-servers">Set custom DNS servers (comma separated)</label>
            <input
                id="dns-servers"
                type="text"
                placeholder="Set custom DNS (comma separated)"
                bind:value={dns}
                data-action="dns"
            />
        </div>
        <button type="button" class="btn" data-action="apply-dns" onclick={applyDns}>
            Apply DNS
        </button>
    </div>

    <div class="form">
        <button
            type="button"
            class="btn ghost danger"
            data-action="forget"
            onclick={() => onForget(network.id)}
        >
            Forget this network
        </button>
    </div>
</div>

<style>
    .detail-head {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 12px 14px;
        border-bottom: 1px solid var(--color-border, #45475a);
    }
    .back {
        cursor: pointer;
        color: var(--color-fg-alt, #a6adc8);
        font-size: 16px;
        background: none;
        border: none;
        padding: 0;
        font-family: inherit;
    }
    .title {
        font-size: 13px;
        font-weight: 600;
        flex: 1;
    }
    .scroll {
        overflow-y: auto;
        flex: 1;
    }
    .adv-toggle {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 10px 14px;
        font-size: 12px;
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
        flex-shrink: 0;
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
    .adv-section-title {
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-fg-alt, #a6adc8);
        padding: 14px 14px 4px;
    }
    .form {
        padding: 4px 14px 10px;
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .field {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .field label {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
    }
    .field input {
        background: var(--color-bg, #1e1e2e);
        border: 1px solid var(--color-border, #45475a);
        color: var(--color-fg, #cdd6f4);
        border-radius: 7px;
        padding: 7px 9px;
        font-size: 12px;
        font-family: inherit;
    }
    .field input:focus {
        outline: none;
        border-color: var(--color-accent, #89b4fa);
    }
    .field input:disabled {
        opacity: 0.5;
    }
    .segmini {
        display: inline-flex;
    }
    .segmini button {
        background: var(--color-bg, #1e1e2e);
        border: 1px solid var(--color-border, #45475a);
        color: var(--color-fg-alt, #a6adc8);
        font-size: 11px;
        padding: 4px 10px;
        cursor: pointer;
        font-family: inherit;
    }
    .segmini button:first-child {
        border-radius: 6px 0 0 6px;
    }
    .segmini button:last-child {
        border-radius: 0 6px 6px 0;
    }
    .segmini button.active {
        background: hsla(217, 92%, 76%, 0.16);
        border-color: var(--color-accent, #89b4fa);
        color: var(--color-accent, #89b4fa);
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
    .btn.ghost {
        background: transparent;
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        font-weight: 400;
    }
    .btn.danger {
        color: var(--color-bad, #f38ba8);
        border-color: var(--color-bad, #f38ba8);
    }
</style>
