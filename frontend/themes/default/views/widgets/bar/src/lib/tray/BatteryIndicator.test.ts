import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import BatteryIndicator from './BatteryIndicator.svelte';
import type { PowerState } from '../types';

function mockClient(): { client: any; emit: (p: PowerState) => Promise<void> } {
    let saved: ((p: unknown) => void) | undefined;
    const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
        saved = cb;
        return () => {};
    });
    const client = { call: vi.fn().mockResolvedValue(undefined), subscribe, close: vi.fn() };
    const emit = async (p: PowerState) => {
        await tick();
        saved?.(p);
        await tick();
    };
    return { client, emit };
}

describe('BatteryIndicator', () => {
    it('renders nothing when state is unavailable', () => {
        const { client } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        expect(container.querySelector('.tray-icon')).toBeNull();
    });

    it('renders the percentage glyph when available and discharging', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emit({
            available: true,
            on_battery: true,
            percentage: 50,
            state: 'discharging',
            time_to_empty_secs: 1800,
            time_to_full_secs: null,
        });
        const el = container.querySelector('.tray-icon');
        expect(el).not.toBeNull();
        expect(el!.textContent).toContain('50%');
        expect(el!.textContent).not.toContain('⚡');
    });

    it('shows charging glyph when state is charging', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emit({
            available: true,
            on_battery: false,
            percentage: 80,
            state: 'charging',
            time_to_empty_secs: null,
            time_to_full_secs: 1200,
        });
        const el = container.querySelector('.tray-icon');
        expect(el!.textContent).toContain('⚡');
        expect(el!.textContent).toContain('80%');
    });

    it('tooltip includes time remaining when discharging', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emit({
            available: true,
            on_battery: true,
            percentage: 25,
            state: 'discharging',
            time_to_empty_secs: 600,
            time_to_full_secs: null,
        });
        const el = container.querySelector('.tray-icon');
        expect(el!.getAttribute('title')).toContain('10 min to empty');
    });
});
