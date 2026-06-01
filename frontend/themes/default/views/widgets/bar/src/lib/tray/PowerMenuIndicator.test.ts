import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import PowerMenuIndicator from './PowerMenuIndicator.svelte';
import type { SystemPowerState } from '../types';

function mockClient() {
    let saved: ((p: unknown) => void) | undefined;
    const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
        saved = cb;
        return () => {};
    });
    const call = vi.fn().mockResolvedValue(undefined);
    const client = { call, subscribe, close: vi.fn() };
    const emit = async (p: SystemPowerState) => {
        await tick();
        saved?.(p);
        await tick();
    };
    return { client, emit };
}

describe('PowerMenuIndicator', () => {
    it('renders nothing when no capabilities are available', () => {
        const { client } = mockClient();
        const { container } = render(PowerMenuIndicator, { props: { client } });
        expect(container.querySelector('.power-menu-trigger')).toBeNull();
    });

    it('renders the power icon button when at least one capability is true', async () => {
        const { client, emit } = mockClient();
        const { container } = render(PowerMenuIndicator, { props: { client } });
        await emit({
            can_shutdown: false,
            can_restart: false,
            can_suspend: true,
            can_hibernate: false,
            can_lock: false,
        });
        const button = container.querySelector('.power-menu-trigger');
        expect(button).not.toBeNull();
        expect(button!.querySelector('svg.icon')).not.toBeNull();
    });

    it('click dispatches view.show widgets/power-menu', async () => {
        const { client, emit } = mockClient();
        const { container } = render(PowerMenuIndicator, { props: { client } });
        await emit({
            can_shutdown: true,
            can_restart: true,
            can_suspend: true,
            can_hibernate: false,
            can_lock: true,
        });
        const button = container.querySelector('.power-menu-trigger') as HTMLElement;
        await fireEvent.click(button);
        await tick();
        expect(client.call).toHaveBeenCalledWith('view.show', { name: 'widgets/power-menu' });
    });
});
