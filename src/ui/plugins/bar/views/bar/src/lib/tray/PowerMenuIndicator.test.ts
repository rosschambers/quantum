import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import { closeContextMenu } from '@quantum/client';
import PowerMenuIndicator from './PowerMenuIndicator.svelte';
import type { SystemPowerState } from '../types';

function menuItem(text: string): HTMLButtonElement | undefined {
    return Array.from(
        document.querySelectorAll('[data-quantum-context-menu] [role="menuitem"]'),
    ).find((el) => el.textContent?.includes(text)) as HTMLButtonElement | undefined;
}

afterEach(() => {
    closeContextMenu();
    (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = undefined;
});

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
        expect(container.querySelector('.bar-button')).toBeNull();
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
        const button = container.querySelector('.bar-button');
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
        const button = container.querySelector('.bar-button') as HTMLElement;
        await fireEvent.click(button);
        await tick();
        expect(client.call).toHaveBeenCalledWith('view.show', { name: 'widgets/power-menu' });
    });

    it('locks the session from the right-click menu', async () => {
        const { client, emit } = mockClient();
        const { container } = render(PowerMenuIndicator, { props: { client } });
        await emit({
            can_shutdown: true,
            can_restart: true,
            can_suspend: true,
            can_hibernate: false,
            can_lock: true,
        });

        const button = container.querySelector('.bar-button') as HTMLElement;
        await fireEvent.contextMenu(button);
        await tick();

        const lock = menuItem('Lock');
        expect(lock).toBeTruthy();
        await fireEvent.click(lock as HTMLButtonElement);
        await tick();

        expect(client.call).toHaveBeenCalledWith('action.invoke', {
            provider: 'system_power',
            action: {
                kind: 'custom',
                data: { kind: 'system_power', payload: { command: 'lock' } },
            },
        });
    });

    it('omits capabilities that are unavailable', async () => {
        const { client, emit } = mockClient();
        const { container } = render(PowerMenuIndicator, { props: { client } });
        await emit({
            can_shutdown: false,
            can_restart: false,
            can_suspend: true,
            can_hibernate: false,
            can_lock: false,
        });

        const button = container.querySelector('.bar-button') as HTMLElement;
        await fireEvent.contextMenu(button);
        await tick();

        expect(menuItem('Suspend')).toBeTruthy();
        expect(menuItem('Lock')).toBeUndefined();
        expect(menuItem('Shutdown')).toBeUndefined();
        expect(menuItem('Restart')).toBeUndefined();
    });
});
