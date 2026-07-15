import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import { closeContextMenu } from '@quantum/client';
import SystemMeters from './SystemMeters.svelte';

function contextMenu(): HTMLElement | null {
    return document.querySelector('[data-quantum-context-menu]');
}

function menuItem(text: string): HTMLButtonElement | undefined {
    return Array.from(
        document.querySelectorAll('[data-quantum-context-menu] [role="menuitem"]'),
    ).find((el) => el.textContent?.includes(text)) as HTMLButtonElement | undefined;
}

afterEach(() => {
    closeContextMenu();
});

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
    it('renders two rings with placeholder label and unknown tooltip before any event arrives', () => {
        const { client } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        const cpu = container.querySelector('.meter.cpu');
        const mem = container.querySelector('.meter.mem');
        expect(cpu).not.toBeNull();
        expect(mem).not.toBeNull();
        expect(cpu!.getAttribute('title')).toBe('CPU: unknown');
        expect(mem!.getAttribute('title')).toBe('MEM: unknown');
        const cpuLabel = container.querySelector('.meter.cpu .ring-label');
        const memLabel = container.querySelector('.meter.mem .ring-label');
        expect(cpuLabel!.textContent).toBe('--');
        expect(memLabel!.textContent).toBe('--');
    });

    it('updates ring label and tooltip after an event fires', async () => {
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
        const cpuLabel = container.querySelector('.meter.cpu .ring-label');
        const memLabel = container.querySelector('.meter.mem .ring-label');
        expect(cpuLabel!.textContent).toBe('19');
        expect(memLabel!.textContent).toBe('50');
    });

    it('drives the ring stroke-dashoffset by the current percentage', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        // 0% -> dashoffset == circumference (no fill)
        await emit({
            cpu_percent: 0,
            mem_used_bytes: 5_000_000_000,
            mem_total_bytes: 10_000_000_000,
        });
        const cpuFill0 = container.querySelector('.meter.cpu .ring-fill');
        const circ = Number(cpuFill0!.getAttribute('stroke-dasharray'));
        const off0 = Number(cpuFill0!.getAttribute('stroke-dashoffset'));
        expect(off0).toBeCloseTo(circ, 1);
        // 100% -> dashoffset == 0 (fully filled)
        await emit({
            cpu_percent: 100,
            mem_used_bytes: 5_000_000_000,
            mem_total_bytes: 10_000_000_000,
        });
        const cpuFill100 = container.querySelector('.meter.cpu .ring-fill');
        const off100 = Number(cpuFill100!.getAttribute('stroke-dashoffset'));
        expect(off100).toBeCloseTo(0, 1);
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
        const memLabel = container.querySelector('.meter.mem .ring-label');
        expect(memLabel!.textContent).toBe('--');
    });

    it('color-codes the ring stroke based on the current percentage', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await emit({
            cpu_percent: 0,
            mem_used_bytes: 0,
            mem_total_bytes: 10_000_000_000,
        });
        const cpuFillLow = container.querySelector('.meter.cpu .ring-fill');
        const lowStroke = cpuFillLow!.getAttribute('stroke') ?? '';
        // Cool blue at 0%: low R, high B.
        const matchLow = lowStroke.match(/rgb\((\d+),\s*(\d+),\s*(\d+)\)/);
        expect(matchLow).not.toBeNull();
        const [, lr, , lb] = matchLow!;
        expect(Number(lr)).toBeLessThan(120);
        expect(Number(lb)).toBeGreaterThan(200);

        await emit({
            cpu_percent: 100,
            mem_used_bytes: 0,
            mem_total_bytes: 10_000_000_000,
        });
        const cpuFillHigh = container.querySelector('.meter.cpu .ring-fill');
        const highStroke = cpuFillHigh!.getAttribute('stroke') ?? '';
        const matchHigh = highStroke.match(/rgb\((\d+),\s*(\d+),\s*(\d+)\)/);
        const [, hr, , hb] = matchHigh!;
        expect(Number(hr)).toBeGreaterThan(200);
        expect(Number(hb)).toBeLessThan(120);
    });

    it('renders an empty sparkline before two samples have arrived', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        // 0 samples
        expect(container.querySelector('.meter.cpu .sparkline path')).toBeNull();
        await emit({
            cpu_percent: 30,
            mem_used_bytes: 1_000_000_000,
            mem_total_bytes: 10_000_000_000,
        });
        // 1 sample
        expect(container.querySelector('.meter.cpu .sparkline path')).toBeNull();
        await emit({
            cpu_percent: 70,
            mem_used_bytes: 5_000_000_000,
            mem_total_bytes: 10_000_000_000,
        });
        // 2 samples — path should now appear
        const cpuPath = container.querySelector('.meter.cpu .sparkline path');
        const memPath = container.querySelector('.meter.mem .sparkline path');
        expect(cpuPath).not.toBeNull();
        expect(memPath).not.toBeNull();
    });

    it('draws the sparkline as a smoothed Bezier path (uses C commands)', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await emit({ cpu_percent: 10, mem_used_bytes: 1e9, mem_total_bytes: 1e10 });
        await emit({ cpu_percent: 80, mem_used_bytes: 5e9, mem_total_bytes: 1e10 });
        await emit({ cpu_percent: 40, mem_used_bytes: 3e9, mem_total_bytes: 1e10 });
        const cpuPath = container.querySelector('.meter.cpu .sparkline path');
        const d = cpuPath!.getAttribute('d') ?? '';
        // Should start with a Move and contain Cubic Bezier segments.
        expect(d.startsWith('M')).toBe(true);
        expect(d).toContain('C');
        // Two segments expected for three samples: one C per pair.
        const cubicCount = (d.match(/C/g) ?? []).length;
        expect(cubicCount).toBe(2);
    });

    it('sparkline stroke uses the same gradient color as the ring', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await emit({ cpu_percent: 50, mem_used_bytes: 1e9, mem_total_bytes: 1e10 });
        await emit({ cpu_percent: 50, mem_used_bytes: 1e9, mem_total_bytes: 1e10 });
        const ringStroke = container
            .querySelector('.meter.cpu .ring-fill')!
            .getAttribute('stroke');
        const sparkStroke = container
            .querySelector('.meter.cpu .sparkline path')!
            .getAttribute('stroke');
        expect(sparkStroke).toBe(ringStroke);
    });

    it('opens a quick-actions menu on right-click', async () => {
        const { client } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await tick();

        expect(contextMenu()).toBeNull();
        await fireEvent.contextMenu(container.querySelector('.meters') as HTMLElement);
        await tick();

        expect(contextMenu()).not.toBeNull();
        expect(menuItem('Open Task Manager')).toBeTruthy();
    });

    it('left-click invokes the hyprctl kill window picker directly without opening the menu', async () => {
        const { client } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await tick();

        client.call.mockClear();
        await fireEvent.click(container.querySelector('.meters') as HTMLElement);
        await tick();

        expect(client.call).toHaveBeenCalledWith('action.invoke', {
            provider: 'shell',
            action: {
                kind: 'shell',
                data: { command: ['hyprctl', 'kill'], terminal: false },
            },
        });
        // Left-click triggers the picker directly, it must not open the menu.
        expect(contextMenu()).toBeNull();
    });

    it('toggles the task manager panel from the menu without a monitor suffix', async () => {
        const { client } = mockClient();
        const { container } = render(SystemMeters, { props: { client } });
        await tick();

        await fireEvent.contextMenu(container.querySelector('.meters') as HTMLElement);
        await tick();

        const open = menuItem('Open Task Manager');
        expect(open).toBeTruthy();
        client.call.mockClear();
        await fireEvent.click(open as HTMLButtonElement);
        await tick();

        expect(client.call).toHaveBeenCalledWith('view.toggle', {
            name: 'plugin/task-manager/task-manager',
        });
    });
});
