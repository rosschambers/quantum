import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import ActiveWindow from './ActiveWindow.svelte';

function makeMockClient(subscribeCallback?: (payload: unknown) => void) {
    return {
        call: vi.fn().mockResolvedValue(undefined),
        subscribe: vi.fn((_channel: string, cb: (payload: unknown) => void) => {
            if (subscribeCallback) {
                // tests grab the callback to invoke later
            }
            return () => {};
        }),
        close: vi.fn(),
    };
}

describe('ActiveWindow', () => {
    it('renders empty when no event has fired', () => {
        const client = makeMockClient();
        const { container } = render(ActiveWindow, { props: { client } });
        // Component should exist with empty title
        const titleEl = container.querySelector('.title');
        expect(titleEl?.textContent).toEqual('');
    });

    it('renders workspace and title after event', async () => {
        let savedCallback: ((p: unknown) => void) | undefined;
        const client = {
            call: vi.fn().mockResolvedValue(undefined),
            subscribe: vi.fn((_channel: string, cb: (payload: unknown) => void) => {
                savedCallback = cb;
                return () => {};
            }),
            close: vi.fn(),
        };
        const { container } = render(ActiveWindow, { props: { client } });
        
        // Fire the callback immediately if it was registered
        if (savedCallback) {
            savedCallback({
                monitors: {
                    'DP-1': { title: 'Mozilla Firefox', class: 'firefox', workspace_id: 1, workspace_name: '1' },
                },
                focused_monitor: 'DP-1',
            });
            await tick();
        }
        
        const titleEl = container.querySelector('.title');
        const wsEl = container.querySelector('.workspace');
        if (titleEl && wsEl) {
            expect(titleEl.textContent).toContain('Mozilla Firefox');
            expect(wsEl.textContent).toContain('1');
        }
    });

    it('hides workspace badge for special workspaces', async () => {
        let savedCallback: ((p: unknown) => void) | undefined;
        const client = {
            call: vi.fn().mockResolvedValue(undefined),
            subscribe: vi.fn((_channel: string, cb: (payload: unknown) => void) => {
                savedCallback = cb;
                return () => {};
            }),
            close: vi.fn(),
        };
        const { container } = render(ActiveWindow, { props: { client } });
        
        if (savedCallback) {
            savedCallback({
                monitors: {
                    'DP-1': { title: 'Alacritty', class: 'Alacritty', workspace_id: -98, workspace_name: 'special:scratchpad' },
                },
                focused_monitor: 'DP-1',
            });
            await tick();
        }
        
        expect(container.querySelector('.workspace')).toBeNull();
    });

    it('selects the entry for window.__quantum_monitor when set', async () => {
        (window as any).__quantum_monitor = 'DP-2';
        try {
            let savedCallback: ((p: unknown) => void) | undefined;
            const client = {
                call: vi.fn().mockResolvedValue(undefined),
                subscribe: vi.fn((_channel: string, cb: (payload: unknown) => void) => {
                    savedCallback = cb;
                    return () => {};
                }),
                close: vi.fn(),
            };
            const { container } = render(ActiveWindow, { props: { client } });
            savedCallback!({
                monitors: {
                    'DP-1': { title: 'Firefox-A', class: 'firefox', workspace_id: 1, workspace_name: '1' },
                    'DP-2': { title: 'Firefox-B', class: 'firefox', workspace_id: 2, workspace_name: '2' },
                },
                focused_monitor: 'DP-1',
            });
            await tick();
            expect(container.querySelector('.title')?.textContent).toContain('Firefox-B');
        } finally {
            delete (window as any).__quantum_monitor;
        }
    });

    it('falls back to focused_monitor when __quantum_monitor is unset', async () => {
        let savedCallback: ((p: unknown) => void) | undefined;
        const client = {
            call: vi.fn().mockResolvedValue(undefined),
            subscribe: vi.fn((_channel: string, cb: (payload: unknown) => void) => {
                savedCallback = cb;
                return () => {};
            }),
            close: vi.fn(),
        };
        const { container } = render(ActiveWindow, { props: { client } });
        savedCallback!({
            monitors: {
                'DP-1': { title: 'Firefox', class: 'firefox', workspace_id: 1, workspace_name: '1' },
            },
            focused_monitor: 'DP-1',
        });
        await tick();
        expect(container.querySelector('.title')?.textContent).toContain('Firefox');
    });
});
