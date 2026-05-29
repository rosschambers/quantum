import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import SystemMeters from './SystemMeters.svelte';

describe('SystemMeters', () => {
    it('renders dashes before any event arrives', () => {
        const client = {
            call: vi.fn(),
            subscribe: vi.fn(() => () => {}),
            close: vi.fn(),
        };
        const { container } = render(SystemMeters, { props: { client } });
        expect(container.textContent).toContain('CPU --%');
        expect(container.textContent).toContain('MEM --%');
    });

    it('renders values after an event fires', async () => {
        let savedCallback: ((p: unknown) => void) | undefined;
        const client = {
            call: vi.fn(),
            subscribe: vi.fn((_channel: string, cb: (p: unknown) => void) => {
                savedCallback = cb;
                return () => {};
            }),
            close: vi.fn(),
        };
        const { container } = render(SystemMeters, { props: { client } });
        // Wait for onMount to run
        await new Promise(resolve => setTimeout(resolve, 10));
        savedCallback?.({
            cpu_percent: 18.7,
            mem_used_bytes: 5_000_000_000,
            mem_total_bytes: 10_000_000_000,
        });
        await new Promise(resolve => setTimeout(resolve, 10));
        expect(container.textContent).toContain('CPU 19%');
        expect(container.textContent).toContain('MEM 50%');
    });

    it('handles zero total memory gracefully', async () => {
        let savedCallback: ((p: unknown) => void) | undefined;
        const client = {
            call: vi.fn(),
            subscribe: vi.fn((_channel: string, cb: (p: unknown) => void) => {
                savedCallback = cb;
                return () => {};
            }),
            close: vi.fn(),
        };
        const { container } = render(SystemMeters, { props: { client } });
        // Wait for onMount to run
        await new Promise(resolve => setTimeout(resolve, 10));
        savedCallback?.({
            cpu_percent: 5,
            mem_used_bytes: 0,
            mem_total_bytes: 0,
        });
        await new Promise(resolve => setTimeout(resolve, 10));
        expect(container.textContent).toContain('CPU 5%');
        expect(container.textContent).toContain('MEM --%');
    });
});
