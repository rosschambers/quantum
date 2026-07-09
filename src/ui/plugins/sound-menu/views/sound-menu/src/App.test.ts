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
        // Task 6 shell only renders availability; Task 7 restores the
        // device-content assertion once the device list renders. Here we prove
        // the subscribe callback swaps `state` and the DOM re-renders by
        // flipping availability through the stream. Scoped to `container`
        // because there is no testing-library auto-cleanup configured and
        // body-scoped queries would see prior tests' leftover DOM.
        const { container } = render(App);
        await settle();
        expect(capturedSubscribeCallback).not.toBeNull();
        expect(container.querySelector('.unavailable')).toBeNull();
        capturedSubscribeCallback!(unavailableState());
        await settle();
        expect(container.querySelector('.unavailable')).not.toBeNull();
    });
});
