<script lang="ts">
    import type { BluetoothDevice } from './types';

    interface Props {
        device: BluetoothDevice;
        status: 'pairing' | 'connecting' | 'error' | null;
        actionLabel: string | null;
        onAction: (() => void) | null;
        onSelect: (() => void) | null;
    }

    const { device, status, actionLabel, onAction, onSelect }: Props = $props();

    /** Map the BlueZ icon class to a glyph; text keeps the view font-only. */
    function glyphFor(icon: string | null): string {
        if (icon === null) return '\u2b24'; // generic dot
        if (icon.startsWith('audio')) return '\u266b';
        if (icon.startsWith('input')) return '\u2328';
        if (icon.startsWith('phone')) return '\u260e';
        return '\u2b24';
    }

    function onRowClick(): void {
        onSelect?.();
    }

    function onActionClick(event: MouseEvent): void {
        event.stopPropagation();
        onAction?.();
    }

    function onRowKeyDown(event: KeyboardEvent): void {
        if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            onSelect?.();
        }
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
    {#if actionLabel !== null}
        <button
            type="button"
            class="row-action"
            data-action={actionLabel.toLowerCase()}
            onclick={onActionClick}
        >
            {actionLabel}
        </button>
    {/if}
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
    .row-status {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
    }
    .row-status.error {
        color: var(--color-danger, #f38ba8);
    }
    .row-action {
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 6px;
        padding: 3px 8px;
        font-size: 11px;
        cursor: pointer;
        font-family: inherit;
    }
    .row-action:hover {
        border-color: var(--color-accent, #89b4fa);
        color: var(--color-accent, #89b4fa);
    }
</style>
