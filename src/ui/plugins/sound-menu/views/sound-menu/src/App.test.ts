import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import type { AudioState } from './lib/types';
import type { MenuItem } from '@quantum/client';

let mockState: AudioState;
let mockCallSpy = vi.fn();
let mockSubscribeSpy = vi.fn();
let capturedSubscribeCallback: ((payload: unknown) => void) | null = null;
let capturedMenuItems: MenuItem[] | null = null;

export function fixtureState(): AudioState {
    return {
        available: true,
        default_sink: { name: 'speaker-sink', description: 'Speaker', volume_percent: 55, muted: false },
        default_source: { name: 'microphone-source', description: 'Digital Microphone', volume_percent: 100, muted: false },
        sinks: [
            { index: 59, name: 'speaker-sink', description: 'Speaker', volume_percent: 55, muted: false, is_default: true, port: 'Speaker' },
            { index: 56, name: 'hdmi-sink', description: 'HDMI / DisplayPort 3 Output', volume_percent: 100, muted: true, is_default: false, port: 'HDMI / DisplayPort 3 Output' },
        ],
        sources: [
            { index: 61, name: 'microphone-source', description: 'Digital Microphone', volume_percent: 100, muted: false, is_default: true, port: 'Digital Microphone' },
        ],
        playback_streams: [
            { index: 900, application_name: 'Firefox', media_name: 'Song Title', icon: null, volume_percent: 80, muted: false, device_index: 59 },
        ],
        recording_streams: [],
        cards: [
            {
                index: 48,
                name: 'alsa-card',
                description: 'Arrow Lake cAVS',
                active_profile: 'HiFi',
                profiles: [
                    { name: 'HiFi', description: 'Play HiFi quality Music', available: true },
                    { name: 'off', description: 'Off', available: true },
                ],
            },
        ],
    };
}

function unavailableState(): AudioState {
    return {
        available: false,
        default_sink: null,
        default_source: null,
        sinks: [],
        sources: [],
        playback_streams: [],
        recording_streams: [],
        cards: [],
    };
}

vi.mock('@quantum/client', () => ({
    createClient: () => ({
        call: (...callArguments: unknown[]) => {
            mockCallSpy(...callArguments);
            const [method, parameters] = callArguments as [string, unknown];
            if (method === 'provider.query' && (parameters as { id?: string })?.id === 'audio') {
                return Promise.resolve(mockState);
            }
            return Promise.resolve(undefined);
        },
        subscribe: (...subscribeArguments: unknown[]) => {
            mockSubscribeSpy(...subscribeArguments);
            const [, callback] = subscribeArguments as [string, (payload: unknown) => void];
            capturedSubscribeCallback = callback;
            return () => {};
        },
        close: vi.fn(),
    }),
    openContextMenu: (_event: MouseEvent, items: MenuItem[]) => {
        capturedMenuItems = items;
    },
    __esModule: true,
}));

import App from './App.svelte';

beforeEach(() => {
    mockCallSpy = vi.fn();
    mockSubscribeSpy = vi.fn();
    capturedSubscribeCallback = null;
    capturedMenuItems = null;
    mockState = fixtureState();
});

async function settle(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 10));
    await tick();
    await tick();
}

/** The action.invoke calls recorded by the spy, newest last. */
function invokeCalls(): Array<{
    provider?: string;
    action?: { kind?: string; data?: { kind?: string; payload?: Record<string, unknown> } };
}> {
    return mockCallSpy.mock.calls
        .filter(([method]) => method === 'action.invoke')
        .map(([, parameters]) => parameters as never);
}

/** Find the most recent action.invoke whose payload.command matches. */
function lastCommand(command: string):
    | { kind?: string; data?: { kind?: string; payload?: Record<string, unknown> } }
    | undefined {
    const matches = invokeCalls().filter(
        (parameters) => parameters.action?.data?.payload?.command === command,
    );
    return matches.length > 0 ? matches[matches.length - 1].action : undefined;
}

describe('SoundMenu App shell', () => {
    it('opens a provider session on mount with the exact envelope', async () => {
        render(App);
        await settle();
        const call = mockCallSpy.mock.calls.find(
            ([method, parameters]) =>
                method === 'action.invoke' &&
                (parameters as { action?: { data?: { payload?: { command?: string } } } }).action
                    ?.data?.payload?.command === 'open_session',
        );
        expect(call).toBeDefined();
        const [, parameters] = call!;
        expect(parameters).toEqual({
            provider: 'audio',
            action: {
                kind: 'custom',
                data: { kind: 'audio', payload: { command: 'open_session' } },
            },
        });
    });

    it('Escape closes the session and hides by the canonical view name, in that order', async () => {
        render(App);
        await settle();
        mockCallSpy.mockClear();
        await fireEvent.keyDown(document, { key: 'Escape' });
        await tick();
        expect(lastCommand('close_session')).toBeDefined();
        expect(mockCallSpy).toHaveBeenCalledWith('view.hide', {
            name: 'plugin/sound-menu/sound-menu',
        });
        // close_session must be sent before view.hide: the overlay webview is
        // kept warm, so unmount cleanup does not run on dismiss.
        const methods = mockCallSpy.mock.calls.map(([method]) => method);
        expect(methods.indexOf('action.invoke')).toBeLessThan(methods.indexOf('view.hide'));
    });

    it('backdrop click dismisses; clicks inside the card do not', async () => {
        const { container } = render(App);
        await settle();
        mockCallSpy.mockClear();
        await fireEvent.click(container.querySelector('.card')!);
        await tick();
        expect(mockCallSpy).not.toHaveBeenCalledWith('view.hide', {
            name: 'plugin/sound-menu/sound-menu',
        });
        await fireEvent.click(container.querySelector('.backdrop')!);
        await tick();
        expect(mockCallSpy).toHaveBeenCalledWith('view.hide', {
            name: 'plugin/sound-menu/sound-menu',
        });
    });

    it('renders a service-unavailable card when the provider is unavailable', async () => {
        mockState = unavailableState();
        const { getByText } = render(App);
        await settle();
        expect(getByText(/audio service unavailable/i)).not.toBeNull();
    });

    it('re-renders when a new AudioState arrives on the stream', async () => {
        const { container } = render(App);
        await settle();
        expect(capturedSubscribeCallback).not.toBeNull();
        const updated = fixtureState();
        updated.sinks = [
            { index: 77, name: 'fresh-sink', description: 'Freshly Plugged', volume_percent: 30, muted: false, is_default: true, port: null },
        ];
        capturedSubscribeCallback!(updated);
        await settle();
        expect(container.textContent).toContain('Freshly Plugged');
    });
});

describe('SoundMenu device sections', () => {
    it('renders every sink with description and active port subtitle', async () => {
        const { container } = render(App);
        await settle();
        const outputs = container.querySelector('[data-section="outputs"]')!;
        expect(outputs.textContent).toContain('Speaker');
        expect(outputs.textContent).toContain('HDMI / DisplayPort 3 Output');
        const speakerRow = outputs.querySelector('[data-device-name="speaker-sink"]')!;
        expect(speakerRow.querySelector('.device-port')!.textContent).toContain('Speaker');
    });

    it('clicking a non-default sink radio sends the exact set_default_sink envelope', async () => {
        const { container } = render(App);
        await settle();
        const radio = container.querySelector(
            '[data-device-name="hdmi-sink"] [data-action="set-default"]',
        ) as HTMLInputElement;
        await fireEvent.click(radio);
        await tick();
        const action = lastCommand('set_default_sink');
        expect(action).toEqual({
            kind: 'custom',
            data: { kind: 'audio', payload: { command: 'set_default_sink', name: 'hdmi-sink' } },
        });
    });

    it('clicking the already-default radio sends nothing', async () => {
        const { container } = render(App);
        await settle();
        mockCallSpy.mockClear();
        const radio = container.querySelector(
            '[data-device-name="speaker-sink"] [data-action="set-default"]',
        ) as HTMLInputElement;
        await fireEvent.click(radio);
        await tick();
        expect(lastCommand('set_default_sink')).toBeUndefined();
    });

    it('sink mute button sends set_device_mute with the inverted flag', async () => {
        const { container } = render(App);
        await settle();
        // hdmi-sink is muted in the fixture, so the toggle unmutes.
        const muteButton = container.querySelector(
            '[data-device-name="hdmi-sink"] [data-action="mute"]',
        ) as HTMLButtonElement;
        await fireEvent.click(muteButton);
        await tick();
        expect(lastCommand('set_device_mute')!.data!.payload).toEqual({
            command: 'set_device_mute',
            kind: 'sink',
            name: 'hdmi-sink',
            muted: false,
        });
    });

    it('source rows send set_default_source and set_device_mute with kind source', async () => {
        const { container } = render(App);
        await settle();
        const inputs = container.querySelector('[data-section="inputs"]')!;
        const muteButton = inputs.querySelector(
            '[data-device-name="microphone-source"] [data-action="mute"]',
        ) as HTMLButtonElement;
        await fireEvent.click(muteButton);
        await tick();
        expect(lastCommand('set_device_mute')!.data!.payload).toEqual({
            command: 'set_device_mute',
            kind: 'source',
            name: 'microphone-source',
            muted: true,
        });
    });

    it('releasing a sink slider sends set_device_volume with the slider value', async () => {
        const { container } = render(App);
        await settle();
        const slider = container.querySelector(
            '[data-device-name="speaker-sink"] input[type="range"]',
        ) as HTMLInputElement;
        await fireEvent.input(slider, { target: { value: '70' } });
        await fireEvent.change(slider, { target: { value: '70' } });
        await tick();
        expect(lastCommand('set_device_volume')!.data!.payload).toEqual({
            command: 'set_device_volume',
            kind: 'sink',
            name: 'speaker-sink',
            percent: 70,
        });
    });
});

describe('SoundMenu stream sections', () => {
    it('renders a playback row with application and media names', async () => {
        const { container } = render(App);
        await settle();
        const playback = container.querySelector('[data-section="playback"]')!;
        const row = playback.querySelector('[data-stream-index="900"]')!;
        expect(row.textContent).toContain('Firefox');
        expect(row.textContent).toContain('Song Title');
    });

    it('shows "Nothing playing" when there are no playback streams', async () => {
        mockState = fixtureState();
        mockState.playback_streams = [];
        const { container } = render(App);
        await settle();
        const playback = container.querySelector('[data-section="playback"]')!;
        expect(playback.textContent).toContain('Nothing playing');
    });

    it('hides the recording section entirely when nothing records', async () => {
        const { container } = render(App);
        await settle();
        expect(container.querySelector('[data-section="recording"]')).toBeNull();
    });

    it('shows the recording section when a source-output exists', async () => {
        mockState = fixtureState();
        mockState.recording_streams = [
            { index: 932, application_name: 'OBS', media_name: 'Microphone capture', icon: null, volume_percent: 100, muted: false, device_index: 61 },
        ];
        const { container } = render(App);
        await settle();
        const recording = container.querySelector('[data-section="recording"]')!;
        expect(recording.textContent).toContain('OBS');
    });

    it('stream mute sends set_stream_mute with kind playback', async () => {
        const { container } = render(App);
        await settle();
        const muteButton = container.querySelector(
            '[data-stream-index="900"] [data-action="mute"]',
        ) as HTMLButtonElement;
        await fireEvent.click(muteButton);
        await tick();
        expect(lastCommand('set_stream_mute')!.data!.payload).toEqual({
            command: 'set_stream_mute',
            kind: 'playback',
            index: 900,
            muted: true,
        });
    });

    it('releasing a stream slider sends set_stream_volume', async () => {
        const { container } = render(App);
        await settle();
        const slider = container.querySelector(
            '[data-stream-index="900"] input[type="range"]',
        ) as HTMLInputElement;
        await fireEvent.input(slider, { target: { value: '65' } });
        await fireEvent.change(slider, { target: { value: '65' } });
        await tick();
        expect(lastCommand('set_stream_volume')!.data!.payload).toEqual({
            command: 'set_stream_volume',
            kind: 'playback',
            index: 900,
            percent: 65,
        });
    });

    it('the device dropdown lists sinks and selecting one sends the exact move_stream envelope', async () => {
        const { container } = render(App);
        await settle();
        const pickButton = container.querySelector(
            '[data-stream-index="900"] [data-action="pick-device"]',
        ) as HTMLButtonElement;
        await fireEvent.click(pickButton);
        await tick();
        expect(capturedMenuItems).not.toBeNull();
        expect(capturedMenuItems!.map((item) => item.label)).toEqual([
            'Speaker',
            'HDMI / DisplayPort 3 Output',
        ]);
        capturedMenuItems![1].onSelect!();
        await tick();
        const call = mockCallSpy.mock.calls.find(
            ([method, parameters]) =>
                method === 'action.invoke' &&
                (parameters as { action?: { data?: { payload?: { command?: string } } } }).action
                    ?.data?.payload?.command === 'move_stream',
        );
        expect(call).toBeDefined();
        const [, parameters] = call!;
        expect(parameters).toEqual({
            provider: 'audio',
            action: {
                kind: 'custom',
                data: {
                    kind: 'audio',
                    payload: {
                        command: 'move_stream',
                        kind: 'playback',
                        index: 900,
                        device_name: 'hdmi-sink',
                    },
                },
            },
        });
    });

    it('a recording row dropdown lists sources and fires move_stream with kind record', async () => {
        mockState = fixtureState();
        mockState.recording_streams = [
            { index: 932, application_name: 'OBS', media_name: 'Microphone capture', icon: null, volume_percent: 100, muted: false, device_index: 61 },
        ];
        const { container } = render(App);
        await settle();
        const pickButton = container.querySelector(
            '[data-section="recording"] [data-stream-index="932"] [data-action="pick-device"]',
        ) as HTMLButtonElement;
        await fireEvent.click(pickButton);
        await tick();
        expect(capturedMenuItems!.map((item) => item.label)).toEqual(['Digital Microphone']);
        capturedMenuItems![0].onSelect!();
        await tick();
        expect(lastCommand('move_stream')!.data!.payload).toEqual({
            command: 'move_stream',
            kind: 'record',
            index: 932,
            device_name: 'microphone-source',
        });
    });
});
