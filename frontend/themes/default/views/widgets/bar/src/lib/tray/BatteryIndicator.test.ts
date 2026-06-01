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

    it('renders an icon and a ring when available and discharging', async () => {
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
        const icon = el!.querySelector('.icon');
        expect(icon).not.toBeNull();
        expect(icon!.textContent).not.toBe('');
        // Battery icon while discharging is the plain battery glyph (no
        // lightning prefix). Tooltip carries the percentage.
        expect(icon!.textContent).not.toContain('\u26a1');
        expect(el!.querySelector('svg.ring')).not.toBeNull();
        expect(el!.getAttribute('title')).toContain('50%');
    });

    it('uses the lightning icon when charging', async () => {
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
        const icon = container.querySelector('.tray-icon .icon');
        expect(icon!.textContent).toContain('\u26a1');
        expect(container.querySelector('.tray-icon')!.getAttribute('title')).toContain('80%');
    });

    it('drives the ring fill by the current percentage', async () => {
        const { client, emit } = mockClient();
        const { container } = render(BatteryIndicator, { props: { client } });
        await emit({
            available: true,
            on_battery: true,
            percentage: 100,
            state: 'full',
            time_to_empty_secs: null,
            time_to_full_secs: null,
        });
        const fill = container.querySelector('.tray-icon svg.ring .ring-fill');
        const circ = Number(fill!.getAttribute('stroke-dasharray'));
        const off = Number(fill!.getAttribute('stroke-dashoffset'));
        // At 100% the dashoffset is 0 (ring fully drawn).
        expect(off).toBeCloseTo(0, 1);
        expect(circ).toBeGreaterThan(0);
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
