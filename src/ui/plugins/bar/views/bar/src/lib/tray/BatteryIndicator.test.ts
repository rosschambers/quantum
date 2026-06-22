import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import { closeContextMenu } from '@quantum/client';
import BatteryIndicator from './BatteryIndicator.svelte';
import type { PowerState, PowerProfileState } from '../types';
import { POWER_CHANNEL, POWER_PROFILE_CHANNEL } from '../channels';

function menuItem(text: string): HTMLButtonElement | undefined {
    return Array.from(
        document.querySelectorAll('[data-quantum-context-menu] [role="menuitem"]'),
    ).find((el) => el.textContent?.includes(text)) as HTMLButtonElement | undefined;
}

interface Mock {
    client: any;
    emitBattery: (p: PowerState) => Promise<void>;
    emitProfile: (p: PowerProfileState) => Promise<void>;
}

function mockClient(): Mock {
    const handlers: Record<string, (p: unknown) => void> = {};
    const subscribe = vi.fn((ch: string, cb: (p: unknown) => void) => {
        handlers[ch] = cb;
        return () => {};
    });
    const call = vi.fn().mockResolvedValue(undefined);
    const client = { call, subscribe, close: vi.fn() };
    const emitBattery = async (p: PowerState) => {
        await tick();
        handlers[POWER_CHANNEL]?.(p);
        await tick();
    };
    const emitProfile = async (p: PowerProfileState) => {
        await tick();
        handlers[POWER_PROFILE_CHANNEL]?.(p);
        await tick();
    };
    return { client, emitBattery, emitProfile };
}

const BALANCED: PowerProfileState = {
    available: true,
    active: 'balanced',
    profiles: ['power_saver', 'balanced', 'performance'],
    performance_inhibited: null,
};

describe('BatteryIndicator', () => {
    beforeEach(() => {
        (window as any).__quantum_monitor = undefined;
    });
    afterEach(() => {
        closeContextMenu();
        (window as any).__quantum_monitor = undefined;
    });

    it('renders nothing when battery percentage is null', async () => {
        const { client, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        // Profile alone is not enough — without a battery percentage there
        // is nothing to put inside the ring.
        await emitProfile(BALANCED);
        expect(container.querySelector('.bar-button')).toBeNull();
    });

    it('renders a button with the ring and the profile icon when battery is known', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: true,
            percentage: 50,
            state: 'discharging',
            time_to_empty_secs: 1800,
            time_to_full_secs: null,
        });
        await emitProfile(BALANCED);
        const btn = container.querySelector('.bar-button');
        expect(btn).not.toBeNull();
        // The ring (outer) shows battery percentage.
        expect(btn!.querySelector('svg.ring')).not.toBeNull();
        // The centered icon shows the profile (balanced glyph), not a
        // battery glyph. The Icon component renders inside .icon-overlay.
        expect(btn!.querySelector('.icon-overlay svg.icon')).not.toBeNull();
    });

    it('falls back to the balanced icon when the profile is unavailable', async () => {
        const { client, emitBattery } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: true,
            percentage: 75,
            state: 'discharging',
            time_to_empty_secs: 3600,
            time_to_full_secs: null,
        });
        const btn = container.querySelector('.bar-button');
        expect(btn).not.toBeNull();
        // Still renders with an icon overlay — `powerProfileIcon(null)`
        // resolves to 'balanced'.
        expect(btn!.querySelector('.icon-overlay svg.icon')).not.toBeNull();
    });

    it('drives the ring fill by battery percentage', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: true,
            percentage: 100,
            state: 'full',
            time_to_empty_secs: null,
            time_to_full_secs: null,
        });
        await emitProfile(BALANCED);
        const fill = container.querySelector('.bar-button svg.ring .ring-fill');
        const circ = Number(fill!.getAttribute('stroke-dasharray'));
        const off = Number(fill!.getAttribute('stroke-dashoffset'));
        expect(off).toBeCloseTo(0, 1);
        expect(circ).toBeGreaterThan(0);
    });

    it('shows a bolt overlay when the battery is charging', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: false,
            percentage: 80,
            state: 'charging',
            time_to_empty_secs: null,
            time_to_full_secs: 1200,
        });
        await emitProfile(BALANCED);
        const overlay = container.querySelector('.charging-bolt');
        expect(overlay).not.toBeNull();
        // It is an SVG <polygon> matching the lightning shape.
        expect(overlay!.querySelector('polygon')).not.toBeNull();
    });

    it('hides the bolt overlay when not charging', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: true,
            percentage: 60,
            state: 'discharging',
            time_to_empty_secs: 1800,
            time_to_full_secs: null,
        });
        await emitProfile(BALANCED);
        expect(container.querySelector('.charging-bolt')).toBeNull();
    });

    it('tooltip combines profile name, percentage and time to empty', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: true,
            percentage: 93,
            state: 'discharging',
            // 5h 14m == 5*3600 + 14*60 == 18840
            time_to_empty_secs: 18840,
            time_to_full_secs: null,
        });
        await emitProfile(BALANCED);
        const btn = container.querySelector('.bar-button');
        const title = btn!.getAttribute('title') ?? '';
        expect(title).toContain('Balanced');
        expect(title).toContain('93%');
        expect(title).toContain('5h 14m');
        expect(title).toContain('to empty');
    });

    it('tooltip uses "to full" and time_to_full_secs when charging', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: false,
            percentage: 42,
            state: 'charging',
            time_to_empty_secs: null,
            // 600s == 10m. Under one hour: formatted as "10m".
            time_to_full_secs: 600,
        });
        await emitProfile(BALANCED);
        const title = container.querySelector('.bar-button')!.getAttribute('title') ?? '';
        expect(title).toContain('Balanced');
        expect(title).toContain('42%');
        expect(title).toContain('10m');
        expect(title).toContain('to full');
    });

    it('tooltip shows "charging" without time when no time_to_full is known', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: false,
            percentage: 100,
            state: 'charging',
            time_to_empty_secs: null,
            time_to_full_secs: null,
        });
        await emitProfile(BALANCED);
        const title = container.querySelector('.bar-button')!.getAttribute('title') ?? '';
        expect(title).toContain('Balanced');
        expect(title).toContain('charging');
    });

    it('tooltip falls back to "AC" when not on battery and not charging', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: false,
            percentage: 100,
            state: 'full',
            time_to_empty_secs: null,
            time_to_full_secs: null,
        });
        await emitProfile(BALANCED);
        const title = container.querySelector('.bar-button')!.getAttribute('title') ?? '';
        expect(title).toContain('Balanced');
        expect(title).toContain('AC');
    });

    it('click opens the power-profile-menu view', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: true,
            percentage: 50,
            state: 'discharging',
            time_to_empty_secs: 1800,
            time_to_full_secs: null,
        });
        await emitProfile(BALANCED);
        const btn = container.querySelector('.bar-button');
        expect(btn).not.toBeNull();
        await fireEvent.click(btn!);
        await tick();
        expect(client.call).toHaveBeenCalledWith('view.show', {
            name: 'widgets/power-profile-menu',
        });
    });

    it('click appends the @monitor suffix when __quantum_monitor is set', async () => {
        (window as any).__quantum_monitor = 'DP-1';
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: true,
            percentage: 50,
            state: 'discharging',
            time_to_empty_secs: 1800,
            time_to_full_secs: null,
        });
        await emitProfile(BALANCED);
        const btn = container.querySelector('.bar-button');
        await fireEvent.click(btn!);
        await tick();
        expect(client.call).toHaveBeenCalledWith('view.show', {
            name: 'widgets/power-profile-menu@DP-1',
        });
    });

    it('switches the power profile from the right-click menu', async () => {
        const { client, emitBattery, emitProfile } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emitBattery({
            available: true,
            on_battery: true,
            percentage: 50,
            state: 'discharging',
            time_to_empty_secs: 1800,
            time_to_full_secs: null,
        });
        await emitProfile(BALANCED);

        const btn = container.querySelector('.bar-button') as HTMLButtonElement;
        await fireEvent.contextMenu(btn);
        await tick();

        const performance = menuItem('Performance');
        expect(performance).toBeTruthy();
        await fireEvent.click(performance as HTMLButtonElement);
        await tick();

        expect(client.call).toHaveBeenCalledWith('action.invoke', {
            provider: 'power_profile',
            action: {
                kind: 'custom',
                data: {
                    kind: 'power_profile',
                    payload: { command: 'set', profile: 'performance' },
                },
            },
        });
    });
});
