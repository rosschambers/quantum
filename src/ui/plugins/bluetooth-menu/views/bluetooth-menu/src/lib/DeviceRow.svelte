<script lang="ts">
    import type { BluetoothDevice } from './types';

    export interface DeviceAction {
        key: string;
        glyph: string;
        title: string;
        danger?: boolean;
        onSelect: () => void;
    }

    interface Props {
        device: BluetoothDevice;
        status: 'pairing' | 'connecting' | 'error' | null;
        actions: DeviceAction[];
        onSelect: (() => void) | null;
    }

    const { device, status, actions, onSelect }: Props = $props();

    /** Map the BlueZ icon class to a Unicode entity glyph (never emoji). */
    function glyphFor(icon: string | null): string {
        if (icon === null) return '\u2b24'; // generic filled circle
        if (icon.startsWith('audio')) return '\u266b'; // beamed eighth notes
        if (icon === 'input-keyboard') return '\u2328'; // keyboard
        if (icon === 'input-mouse') return '\u2b24'; // generic circle; no reliable text-presentation mouse glyph
        if (icon === 'input-gamepad' || icon === 'input-gaming') return '\u2b23'; // hexagon
        if (icon.startsWith('input')) return '\u2328';
        if (icon.startsWith('phone')) return '\u260e'; // telephone
        return '\u2b24';
    }

    function onRowClick(): void {
        onSelect?.();
    }

    function onRowKeyDown(event: KeyboardEvent): void {
        if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            onSelect?.();
        }
    }

    function onActionClick(event: MouseEvent, action: DeviceAction): void {
        event.stopPropagation();
        action.onSelect();
    }

    function rssiLabel(rssi: number | null): string {
        return rssi === null ? '' : `${rssi} dBm`;
    }
</script>

<div
    class="device-row"
    class:clickable={onSelect !== null}
    data-address={device.address}
    role="button"
    tabindex="0"
    onclick={onRowClick}
    onkeydown={onRowKeyDown}
>
    <span class="glyph">{glyphFor(device.icon)}</span>
    <span class="name">{device.name === '' ? device.address : device.name}</span>
    {#if device.rssi !== null}
        <span class="rssi" title="Signal strength">{rssiLabel(device.rssi)}</span>
    {/if}
    {#if device.battery_percent !== null}
        <span class="battery">{device.battery_percent}%</span>
    {/if}
    {#if status === 'pairing'}
        <span class="row-status">Pairing...</span>
    {:else if status === 'connecting'}
        <span class="row-status">Connecting...</span>
    {:else if status === 'error'}
        <span class="row-status error">Failed</span>
    {/if}
    {#each actions as action (action.key)}
        <button
            type="button"
            class="row-action"
            class:danger={action.danger}
            data-action={action.key}
            title={action.title}
            aria-label={action.title}
            onclick={(event) => onActionClick(event, action)}
        >
            {action.glyph}
        </button>
    {/each}
</div>

<style>
    .device-row {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 8px 10px;
        border-radius: 8px;
        font-size: 12px;
    }
    .device-row.clickable {
        cursor: pointer;
    }
    .device-row:hover {
        background: var(--color-bg, #1e1e2e);
    }
    .glyph {
        width: 16px;
        text-align: center;
        color: var(--color-fg-alt, #a6adc8);
    }
    .name {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .battery,
    .rssi,
    .row-status {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
    }
    .row-status.error {
        color: var(--color-error, #f38ba8);
    }
    .row-action {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 26px;
        height: 26px;
        border-radius: 50%;
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        font-size: 13px;
        line-height: 1;
        cursor: pointer;
        font-family: inherit;
    }
    .row-action:hover {
        border-color: var(--color-accent, #89b4fa);
        color: var(--color-accent, #89b4fa);
    }
    .row-action.danger:hover {
        border-color: var(--color-error, #f38ba8);
        color: var(--color-error, #f38ba8);
    }
</style>
