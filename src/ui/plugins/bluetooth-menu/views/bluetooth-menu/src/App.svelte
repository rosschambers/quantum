<script lang="ts">
    import { createClient, openContextMenu, type MenuItem } from '@quantum/client';
    import {
        BLUETOOTH_PROVIDER,
        BLUETOOTH_CHANNEL,
        BLUETOOTH_MENU_VIEW,
    } from './lib/channels';
    import type { BluetoothState, BluetoothDevice, PairingRequest } from './lib/types';
    import DeviceRow from './lib/DeviceRow.svelte';
    import PairingDialog from './lib/PairingDialog.svelte';

    const client = createClient();

    let state: BluetoothState = $state({
        available: false,
        powered: false,
        discovering: false,
        devices: [],
        adapter_path: '',
    });

    /** The live pairing prompt, when the agent has one parked (Task 11). */
    let pairingRequest: PairingRequest | null = $state(null);

    /** Per-device inline status keyed by address (Task 10 renders these). */
    type RowStatus = 'pairing' | 'connecting' | 'error';
    let rowStatus: Record<string, RowStatus> = $state({});

    /** The single command envelope, defined in exactly one place. */
    function send(payload: Record<string, unknown>): Promise<unknown> {
        return client.call('action.invoke', {
            provider: BLUETOOTH_PROVIDER,
            action: { kind: 'custom', data: { kind: 'bluetooth', payload } },
        });
    }

    function sendFireAndForget(payload: Record<string, unknown>): void {
        send(payload).catch((error) => {
            console.error(`bluetooth ${String(payload.command)} failed:`, error);
        });
    }

    $effect(() => {
        client
            .call('provider.query', { id: BLUETOOTH_PROVIDER })
            .then((result: unknown) => {
                if (result) state = result as BluetoothState;
            })
            .catch(() => {});
        const off = client.subscribe(BLUETOOTH_CHANNEL, (payload: unknown) => {
            const discriminated = payload as { event?: string };
            if (discriminated.event === 'pairing_request') {
                pairingRequest = payload as PairingRequest;
                return;
            }
            if (discriminated.event === 'pairing_cancelled') {
                pairingRequest = null;
                return;
            }
            state = payload as BluetoothState;
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
        // Stop discovery explicitly before hiding: destroy_on_dismiss unmount
        // cleanup is not guaranteed to run before the surface disappears, and
        // background discovery churns the radio.
        sendFireAndForget({ command: 'close_session' });
        client.call('view.hide', { name: BLUETOOTH_MENU_VIEW }).catch(() => {});
    }

    function togglePowered(): void {
        sendFireAndForget({ command: 'set_powered', value: !state.powered });
    }

    const connectedDevices = $derived(state.devices.filter((device) => device.connected));
    const knownDevices = $derived(
        state.devices.filter((device) => device.paired && !device.connected),
    );
    /** A discovered device with no usable name is "unknown": empty name or a
     * name that is just its MAC address. These are grouped behind an expander. */
    function isNamed(device: BluetoothDevice): boolean {
        return device.name !== '' && device.name !== device.address;
    }

    const availableDevices = $derived(
        state.devices
            .filter((device) => !device.paired && !device.connected)
            .slice()
            .sort((a, b) => (b.rssi ?? -32768) - (a.rssi ?? -32768)),
    );
    const namedAvailable = $derived(availableDevices.filter(isNamed));
    const unknownAvailable = $derived(availableDevices.filter((device) => !isNamed(device)));

    let unknownOpen = $state(false);

    function setRowStatus(address: string, status: RowStatus | null): void {
        if (status === null) {
            const next = { ...rowStatus };
            delete next[address];
            rowStatus = next;
        } else {
            rowStatus = { ...rowStatus, [address]: status };
        }
    }

    /** Available-row click: pair, then connect, then trust. */
    async function pairConnectTrust(device: BluetoothDevice): Promise<void> {
        setRowStatus(device.address, 'pairing');
        try {
            await send({ command: 'pair', address: device.address });
            setRowStatus(device.address, 'connecting');
            await send({ command: 'connect', address: device.address });
            await send({ command: 'set_trusted', address: device.address, value: true });
            setRowStatus(device.address, null);
        } catch (error) {
            console.error('bluetooth pairing flow failed:', error);
            setRowStatus(device.address, 'error');
        }
    }

    async function connectDevice(device: BluetoothDevice): Promise<void> {
        setRowStatus(device.address, 'connecting');
        try {
            await send({ command: 'connect', address: device.address });
            setRowStatus(device.address, null);
        } catch (error) {
            console.error('bluetooth connect failed:', error);
            setRowStatus(device.address, 'error');
        }
    }

    function disconnectDevice(device: BluetoothDevice): void {
        sendFireAndForget({ command: 'disconnect', address: device.address });
    }

    function deviceMenu(event: MouseEvent, device: BluetoothDevice): void {
        const items: MenuItem[] = [
            {
                label: device.trusted ? 'Revoke trust' : 'Trust device',
                onSelect: () =>
                    sendFireAndForget({
                        command: 'set_trusted',
                        address: device.address,
                        value: !device.trusted,
                    }),
            },
            { separator: true, label: '' },
            {
                label: 'Remove device',
                danger: true,
                onSelect: () =>
                    sendFireAndForget({ command: 'remove', address: device.address }),
            },
        ];
        openContextMenu(event, items);
    }

    function onPairingResponse(
        accept: boolean,
        passkey: number | null,
        pin: string | null,
    ): void {
        if (pairingRequest === null) return;
        const payload: Record<string, unknown> = {
            command: 'pairing_response',
            address: pairingRequest.address,
            accept,
        };
        if (passkey !== null) payload.passkey = passkey;
        if (pin !== null) payload.pin = pin;
        sendFireAndForget(payload);
        pairingRequest = null;
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="Bluetooth">
        <div class="card-head">
            <span class="title">Bluetooth</span>
            {#if state.discovering}
                <span class="spinner" title="Scanning for devices"></span>
            {/if}
            <span class="toggle-pill">
                {state.powered ? 'On' : 'Off'}
                <button
                    type="button"
                    class="switch"
                    class:on={state.powered}
                    data-action="power"
                    aria-label="Toggle Bluetooth"
                    aria-pressed={state.powered}
                    onclick={togglePowered}
                ></button>
            </span>
        </div>

        {#if !state.available}
            <div class="empty">Bluetooth service unavailable.</div>
        {:else if !state.powered}
            <div class="empty">
                Bluetooth is turned off.
                <button type="button" class="btn" data-action="power-on" onclick={togglePowered}>
                    Turn on Bluetooth
                </button>
            </div>
        {:else}
            <div class="scroll">
                {#if pairingRequest !== null}
                    <PairingDialog request={pairingRequest} onRespond={onPairingResponse} />
                {/if}
                {#if connectedDevices.length > 0}
                    <div class="section" data-section="connected">
                        <div class="section-title">Connected</div>
                        {#each connectedDevices as device (device.address)}
                            <!-- svelte-ignore a11y_no_static_element_interactions -->
                            <div oncontextmenu={(event) => { event.preventDefault(); deviceMenu(event, device); }}>
                                <DeviceRow
                                    {device}
                                    status={rowStatus[device.address] ?? null}
                                    actionLabel="Disconnect"
                                    onAction={() => disconnectDevice(device)}
                                    onSelect={null}
                                />
                            </div>
                        {/each}
                    </div>
                {/if}
                {#if knownDevices.length > 0}
                    <div class="section" data-section="known">
                        <div class="section-title">Known devices</div>
                        {#each knownDevices as device (device.address)}
                            <!-- svelte-ignore a11y_no_static_element_interactions -->
                            <div oncontextmenu={(event) => { event.preventDefault(); deviceMenu(event, device); }}>
                                <DeviceRow
                                    {device}
                                    status={rowStatus[device.address] ?? null}
                                    actionLabel="Connect"
                                    onAction={() => void connectDevice(device)}
                                    onSelect={null}
                                />
                            </div>
                        {/each}
                    </div>
                {/if}
                <div class="section" data-section="available">
                    <div class="section-title">
                        Available
                        {#if state.discovering}<span class="spinner"></span>{/if}
                    </div>
                    {#if namedAvailable.length === 0 && unknownAvailable.length === 0}
                        <div class="section-empty">Searching for devices...</div>
                    {/if}
                    {#each namedAvailable as device (device.address)}
                        <DeviceRow
                            {device}
                            status={rowStatus[device.address] ?? null}
                            actionLabel={null}
                            onAction={null}
                            onSelect={() => void pairConnectTrust(device)}
                        />
                    {/each}
                    {#if unknownAvailable.length > 0}
                        <button
                            type="button"
                            class="expander-toggle"
                            data-action="toggle-unknown"
                            aria-expanded={unknownOpen}
                            onclick={() => (unknownOpen = !unknownOpen)}
                        >
                            <span class="expander-caret" class:open={unknownOpen}>{'\u203A'}</span>
                            Unknown devices ({unknownAvailable.length})
                        </button>
                        {#if unknownOpen}
                            {#each unknownAvailable as device (device.address)}
                                <DeviceRow
                                    {device}
                                    status={rowStatus[device.address] ?? null}
                                    actionLabel={null}
                                    onAction={null}
                                    onSelect={() => void pairConnectTrust(device)}
                                />
                            {/each}
                        {/if}
                    {/if}
                </div>
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
        width: min(440px, 92vw);
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
    .scroll {
        overflow-y: auto;
        padding: 6px;
        flex: 1;
    }
    .section-title {
        font-size: 11px;
        font-weight: 600;
        color: var(--color-fg-alt, #a6adc8);
        text-transform: uppercase;
        letter-spacing: 0.06em;
        padding: 10px 10px 4px;
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .section-empty {
        font-size: 12px;
        color: var(--color-fg-alt, #a6adc8);
        padding: 8px 10px 14px;
    }
    .expander-toggle {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        background: none;
        border: none;
        color: var(--color-fg-alt, #a6adc8);
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        padding: 8px 10px;
        cursor: pointer;
        font-family: inherit;
        text-align: left;
    }
    .expander-toggle:hover {
        color: var(--color-fg, #cdd6f4);
    }
    .expander-caret {
        display: inline-block;
        transition: transform 0.15s;
    }
    .expander-caret.open {
        transform: rotate(90deg);
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
</style>
