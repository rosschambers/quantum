<script lang="ts">
    import type { AudioDevice } from './types';
    import VolumeSlider from './VolumeSlider.svelte';

    interface Props {
        device: AudioDevice;
        onSetDefault: () => void;
        onSetVolume: (percent: number) => void;
        onToggleMute: () => void;
    }
    let { device, onSetDefault, onSetVolume, onToggleMute }: Props = $props();

    function onRadioClick(): void {
        if (!device.is_default) onSetDefault();
    }
</script>

<div class="device-row" data-device-name={device.name}>
    <label class="default-select">
        <input
            type="radio"
            checked={device.is_default}
            data-action="set-default"
            onclick={onRadioClick}
        />
        <span class="device-text">
            <span class="device-description">{device.description}</span>
            {#if device.port}
                <span class="device-port">{device.port}</span>
            {/if}
        </span>
    </label>
    <VolumeSlider percent={device.volume_percent} onCommit={onSetVolume} />
    <button
        type="button"
        class="mute-button"
        class:muted={device.muted}
        data-action="mute"
        onclick={onToggleMute}
    >
        {device.muted ? 'Unmute' : 'Mute'}
    </button>
</div>

<style>
    .device-row {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 8px 10px;
        border-radius: 8px;
    }
    .device-row:hover {
        background: var(--color-bg, #1e1e2e);
    }
    .default-select {
        display: flex;
        align-items: center;
        gap: 8px;
        flex: 1;
        min-width: 0;
        cursor: pointer;
    }
    .device-text {
        display: flex;
        flex-direction: column;
        min-width: 0;
    }
    .device-description {
        font-size: 12px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .device-port {
        font-size: 10px;
        color: var(--color-fg-alt, #a6adc8);
    }
    .mute-button {
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 6px;
        padding: 3px 8px;
        font-size: 11px;
        cursor: pointer;
        font-family: inherit;
    }
    .mute-button.muted {
        color: var(--color-accent, #89b4fa);
        border-color: var(--color-accent, #89b4fa);
    }
</style>
