import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import SystemMeters from './SystemMeters.svelte';

function mockClient() {
    let savedCallback: ((p: unknown) => void) | undefined;
    const client = {
        call: vi.fn().mockResolvedValue(undefined),
        subscribe: vi.fn((_channel: string, cb: (p: unknown) => void) => {
            savedCallback = cb;
            return () => {};
        }),
        close: vi.fn(),
    };
    const emit = async (p: unknown) => {
        await new Promise((resolve) => setTimeout(resolve, 10));
        savedCallback?.(p);
        await new Promise((resolve) => setTimeout(resolve, 10));
    };
    return { client, emit };
}

describe('SystemMeters', () => {
    it('renders blank bar buckets and unknown tooltip before any event arrives', () => {
        const { client } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        const cpu = container.querySelector('.meter.cpu');
        const mem = container.querySelector('.meter.mem');
        expect(cpu).not.toBeNull();
        expect(mem).not.toBeNull();
        expect(cpu!.getAttribute('title')).toBe('CPU: unknown');
        expect(mem!.getAttribute('title')).toBe('MEM: unknown');
    });

    it('updates the tooltip percent after an event fires', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await emit({
            cpu_percent: 18.7,
            mem_used_bytes: 5_000_000_000,
            mem_total_bytes: 10_000_000_000,
        });
        const cpu = container.querySelector('.meter.cpu');
        const mem = container.querySelector('.meter.mem');
        expect(cpu!.getAttribute('title')).toBe('CPU: 19%');
        expect(mem!.getAttribute('title')).toBe('MEM: 50%');
    });

    it('renders the low-bucket glyph for cpu under 33 percent', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await emit({
            cpu_percent: 5,
            mem_used_bytes: 9_000_000_000,
            mem_total_bytes: 10_000_000_000,
        });
        const cpu = container.querySelector('.meter.cpu .bar');
        const mem = container.querySelector('.meter.mem .bar');
        // \u25ae filled, \u25af empty
        expect(cpu!.textContent).toBe('\u25ae\u25af\u25af');
        expect(mem!.textContent).toBe('\u25ae\u25ae\u25ae');
    });

    it('handles zero total memory gracefully', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await emit({
            cpu_percent: 5,
            mem_used_bytes: 0,
            mem_total_bytes: 0,
        });
        const cpu = container.querySelector('.meter.cpu');
        const mem = container.querySelector('.meter.mem');
        expect(cpu!.getAttribute('title')).toBe('CPU: 5%');
        expect(mem!.getAttribute('title')).toBe('MEM: unknown');
    });

    it('builds an SVG polyline after two samples land', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await emit({
            cpu_percent: 10,
            mem_used_bytes: 1_000_000_000,
            mem_total_bytes: 10_000_000_000,
        });
        // No polyline yet (only 1 sample).
        expect(container.querySelector('.meter.cpu svg polyline')).toBeNull();
        await emit({
            cpu_percent: 80,
            mem_used_bytes: 2_000_000_000,
            mem_total_bytes: 10_000_000_000,
        });
        const cpuPolyline = container.querySelector('.meter.cpu svg polyline');
        const memPolyline = container.querySelector('.meter.mem svg polyline');
        expect(cpuPolyline).not.toBeNull();
        expect(memPolyline).not.toBeNull();
        const cpuPoints = cpuPolyline!.getAttribute('points') ?? '';
        // Two samples should produce two coordinate pairs.
        expect(cpuPoints.split(' ').length).toBe(2);
    });
});
