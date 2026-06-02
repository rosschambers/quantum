import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import MediaControls from './MediaControls.svelte';

describe('MediaControls', () => {
    it('renders disabled buttons when no player is active', () => {
        const call = vi.fn().mockResolvedValue(undefined);
        const subscribe = vi.fn(() => () => {});
        const client = { call, subscribe, close: vi.fn() };
        const { container } = render(MediaControls, { props: { client } });
        const buttons = container.querySelectorAll('button');
        expect(buttons.length).toBe(3);
        buttons.forEach(b => expect((b as HTMLButtonElement).disabled).toBe(true));
    });

    it('enables buttons when player is active and shows play icon when paused', async () => {
        let savedCallback: ((p: unknown) => void) | undefined;
        const call = vi.fn().mockResolvedValue(undefined);
        const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
            savedCallback = cb;
            return () => {};
        });
        const client = { call, subscribe, close: vi.fn() };
        const { container } = render(MediaControls, { props: { client } });
        
        // Ensure onMount has run
        await tick();
        
        expect(savedCallback).toBeDefined();
        
        savedCallback!({
            player_id: 'spotify',
            title: 'Title',
            artist: 'Artist',
            album: null,
            art_url: null,
            playback_status: 'paused',
            position_micros: null,
            length_micros: null,
        });
        await tick();
        
        const buttons = container.querySelectorAll('button');
        buttons.forEach(b => expect((b as HTMLButtonElement).disabled).toBe(false));
    });

    it('invokes play-pause when middle button clicked', async () => {
        let savedCallback: ((p: unknown) => void) | undefined;
        const call = vi.fn().mockResolvedValue(undefined);
        const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
            savedCallback = cb;
            return () => {};
        });
        const client = { call, subscribe, close: vi.fn() };
        const { container } = render(MediaControls, { props: { client } });
        
        // Wait for onMount to run
        await tick();
        
        if (savedCallback) {
            savedCallback({
                player_id: 'spotify',
                title: 'T', artist: 'A', album: null, art_url: null,
                playback_status: 'playing',
                position_micros: null, length_micros: null,
            });
            await tick();
        }
        
        const playPause = container.querySelectorAll('button')[1];
        await fireEvent.click(playPause);
        
        expect(call).toHaveBeenCalledWith('action.invoke', expect.objectContaining({
            provider: 'mpris',
            action: expect.objectContaining({
                kind: 'custom',
                data: expect.objectContaining({
                    kind: 'mpris',
                    payload: expect.objectContaining({ command: 'play-pause' }),
                }),
            }),
        }));
    });
});
