<script lang="ts">
    import type { AudioStream } from './types';
    import VolumeSlider from './VolumeSlider.svelte';
    import Icon from './Icon.svelte';

    interface Props {
        stream: AudioStream;
        pickLabel: string;
        onSetVolume: (percent: number) => void;
        onToggleMute: () => void;
        onPickDevice: (event: MouseEvent) => void;
    }
    let { stream, pickLabel, onSetVolume, onToggleMute, onPickDevice }: Props = $props();
</script>

<div class="stream-row" data-stream-index={stream.index}>
    <div class="stream-text">
        <span class="stream-application">{stream.application_name}</span>
        {#if stream.media_name}
            <span class="stream-media">{stream.media_name}</span>
        {/if}
    </div>
    <VolumeSlider percent={stream.volume_percent} onCommit={onSetVolume} />
    <button
        type="button"
        class="mute-button"
        class:muted={stream.muted}
        data-action="mute"
        title={stream.muted ? 'Unmute' : 'Mute'}
        aria-label={stream.muted ? 'Unmute' : 'Mute'}
        onclick={onToggleMute}
    >
        <Icon name={stream.muted ? 'speaker-muted' : 'speaker'} />
    </button>
    <button type="button" class="pick-button" data-action="pick-device" onclick={onPickDevice}>
        {pickLabel}
    </button>
</div>

<style>
    .stream-row {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 8px 10px;
        border-radius: 8px;
    }
    .stream-row:hover {
        background: var(--color-bg, #1e1e2e);
    }
    .stream-text {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-width: 0;
    }
    .stream-application {
        font-size: 12px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .stream-media {
        font-size: 10px;
        color: var(--color-fg-alt, #a6adc8);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .mute-button {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 6px;
        cursor: pointer;
        font-family: inherit;
    }
    .pick-button {
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 6px;
        padding: 3px 8px;
        font-size: 11px;
        cursor: pointer;
        font-family: inherit;
        white-space: nowrap;
    }
    .mute-button.muted {
        color: var(--color-accent, #89b4fa);
        border-color: var(--color-accent, #89b4fa);
    }
</style>
