<script lang="ts">
    import { createClient, openContextMenu, type MenuItem } from '@quantum/client';
    import { AUDIO_PROVIDER, AUDIO_CHANNEL } from './lib/channels';
    import type { AudioState, AudioStream } from './lib/types';
    import DeviceRow from './lib/DeviceRow.svelte';
    import StreamRow from './lib/StreamRow.svelte';
    import { friendlyProfileName, profileCountLabel } from './lib/profiles';

    const client = createClient();

    let state: AudioState = $state({
        available: false,
        default_sink: null,
        default_source: null,
        sinks: [],
        sources: [],
        playback_streams: [],
        recording_streams: [],
        cards: [],
    });

    /**
     * The single command envelope. Every action goes through here so the
     * nested custom shape is defined in exactly one place.
     */
    function send(payload: Record<string, unknown>): Promise<unknown> {
        return client.call('action.invoke', {
            provider: AUDIO_PROVIDER,
            action: { kind: 'custom', data: { kind: 'audio', payload } },
        });
    }

    function sendFireAndForget(payload: Record<string, unknown>): void {
        send(payload).catch((error) => {
            console.error(`audio ${String(payload.command)} failed:`, error);
        });
    }

    $effect(() => {
        client
            .call('provider.query', { id: AUDIO_PROVIDER })
            .then((result: unknown) => {
                if (result) state = result as AudioState;
            })
            .catch(() => {});
        const off = client.subscribe(AUDIO_CHANNEL, (payload: unknown) => {
            state = payload as AudioState;
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
        // Stop the stream session explicitly: the overlay webview is kept
        // warm (hidden, not destroyed), so on-unmount cleanup does not run on
        // dismiss. Without this the provider would keep fetching sink-inputs
        // in the background while the window is invisible.
        sendFireAndForget({ command: 'close_session' });
        client.call('view.hide', { name: 'plugin/sound-menu/sound-menu' }).catch(() => {});
    }

    function setDefaultDevice(kind: 'sink' | 'source', name: string): void {
        const command = kind === 'sink' ? 'set_default_sink' : 'set_default_source';
        sendFireAndForget({ command, name });
    }

    function setDeviceVolume(kind: 'sink' | 'source', name: string, percent: number): void {
        sendFireAndForget({ command: 'set_device_volume', kind, name, percent });
    }

    function toggleDeviceMute(kind: 'sink' | 'source', name: string, muted: boolean): void {
        sendFireAndForget({ command: 'set_device_mute', kind, name, muted });
    }

    function setStreamVolume(kind: 'playback' | 'record', index: number, percent: number): void {
        sendFireAndForget({ command: 'set_stream_volume', kind, index, percent });
    }

    function toggleStreamMute(kind: 'playback' | 'record', index: number, muted: boolean): void {
        sendFireAndForget({ command: 'set_stream_mute', kind, index, muted });
    }

    /**
     * Device-picker dropdown for a stream row: one entry per candidate
     * device (sinks for playback, sources for recording), anchored below the
     * button, firing move_stream on selection.
     */
    function openMoveMenu(event: MouseEvent, kind: 'playback' | 'record', stream: AudioStream): void {
        const devices = kind === 'playback' ? state.sinks : state.sources;
        const items: MenuItem[] = devices.map((device) => ({
            label: device.description,
            icon: device.index === stream.device_index ? '\u2022' : undefined,
            onSelect: () =>
                sendFireAndForget({
                    command: 'move_stream',
                    kind,
                    index: stream.index,
                    device_name: device.name,
                }),
        }));
        if (items.length === 0) return;
        openContextMenu(event, items, {
            anchorRect: (event.currentTarget as HTMLElement).getBoundingClientRect(),
        });
    }

    function setCardProfile(cardIndex: number, profile: string): void {
        sendFireAndForget({ command: 'set_card_profile', card_index: cardIndex, profile });
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="Sound">
        <div class="card-head">
            <span class="title">Sound</span>
        </div>
        {#if !state.available}
            <div class="empty unavailable">Audio service unavailable.</div>
        {:else}
            <div class="scroll">
                <div class="section" data-section="outputs">
                    <div class="section-title">Output devices</div>
                    {#each state.sinks as sink (sink.index)}
                        <DeviceRow
                            device={sink}
                            onSetDefault={() => setDefaultDevice('sink', sink.name)}
                            onSetVolume={(percent) => setDeviceVolume('sink', sink.name, percent)}
                            onToggleMute={() => toggleDeviceMute('sink', sink.name, !sink.muted)}
                        />
                    {/each}
                    <p class="device-hint">
                        Don't see your device? A device the system cannot enumerate cannot be
                        routed to — check that the dongle or dock is seated and powered, then
                        look under Device profiles below.
                    </p>
                </div>
                <div class="section" data-section="inputs">
                    <div class="section-title">Input devices</div>
                    {#each state.sources as source (source.index)}
                        <DeviceRow
                            device={source}
                            onSetDefault={() => setDefaultDevice('source', source.name)}
                            onSetVolume={(percent) =>
                                setDeviceVolume('source', source.name, percent)}
                            onToggleMute={() =>
                                toggleDeviceMute('source', source.name, !source.muted)}
                        />
                    {/each}
                </div>
                <div class="section" data-section="playback">
                    <div class="section-title">Playback</div>
                    {#if state.playback_streams.length === 0}
                        <div class="empty">Nothing playing</div>
                    {/if}
                    {#each state.playback_streams as stream (stream.index)}
                        <StreamRow
                            {stream}
                            pickLabel="Output"
                            onSetVolume={(percent) =>
                                setStreamVolume('playback', stream.index, percent)}
                            onToggleMute={() =>
                                toggleStreamMute('playback', stream.index, !stream.muted)}
                            onPickDevice={(event) => openMoveMenu(event, 'playback', stream)}
                        />
                    {/each}
                </div>
                {#if state.recording_streams.length > 0}
                    <div class="section" data-section="recording">
                        <div class="section-title">Recording</div>
                        {#each state.recording_streams as stream (stream.index)}
                            <StreamRow
                                {stream}
                                pickLabel="Input"
                                onSetVolume={(percent) =>
                                    setStreamVolume('record', stream.index, percent)}
                                onToggleMute={() =>
                                    toggleStreamMute('record', stream.index, !stream.muted)}
                                onPickDevice={(event) => openMoveMenu(event, 'record', stream)}
                            />
                        {/each}
                    </div>
                {/if}
                <div class="section" data-section="profiles">
                    <div class="section-title">Device profiles</div>
                    <p class="profiles-note">
                        A card's profile decides what it can do. High quality (A2DP) gives
                        stereo sound but no microphone; Headset turns the microphone on at
                        lower quality. Switching a card's profile is how a device with no
                        usable output (an onboard speaker, or a wireless headset) becomes an
                        output.
                    </p>
                    {#each state.cards as card (card.index)}
                        <div class="profile-row" data-card-index={card.index}>
                            <span class="profile-card-name">{card.description}</span>
                            <select
                                data-action="profile"
                                value={card.active_profile}
                                onchange={(event) =>
                                    setCardProfile(
                                        card.index,
                                        (event.currentTarget as HTMLSelectElement).value,
                                    )}
                            >
                                {#each card.profiles as profile (profile.name)}
                                    <option value={profile.name} disabled={!profile.available}>
                                        {friendlyProfileName(profile.name, profile.description)} — {profileCountLabel(
                                            profile.sink_count,
                                            profile.source_count,
                                        )}
                                    </option>
                                {/each}
                            </select>
                        </div>
                    {/each}
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
        width: min(460px, 92vw);
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
    .scroll {
        overflow-y: auto;
        padding: 6px;
        flex: 1;
    }
    .empty {
        text-align: center;
        color: var(--color-fg-alt, #a6adc8);
        font-size: 12px;
        padding: 40px 20px;
    }
    .unavailable {
        padding: 60px 20px;
    }
    .section {
        padding: 4px 4px 10px;
    }
    .section-title {
        font-size: 11px;
        font-weight: 600;
        color: var(--color-fg-alt, #a6adc8);
        text-transform: uppercase;
        letter-spacing: 0.06em;
        padding: 8px 10px 4px;
    }
    .device-hint {
        margin: 0;
        padding: 6px 10px 2px;
        font-size: 11px;
        line-height: 1.4;
        color: var(--color-fg-alt, #a6adc8);
    }
    .profiles-note {
        margin: 0;
        padding: 0 10px 6px;
        font-size: 11px;
        line-height: 1.4;
        color: var(--color-fg-alt, #a6adc8);
    }
    .profile-row {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 8px 10px;
    }
    .profile-card-name {
        font-size: 12px;
        flex: 1;
        min-width: 0;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .profile-row select {
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 6px;
        padding: 3px 8px;
        font-size: 11px;
        font-family: inherit;
        max-width: 55%;
    }
</style>
