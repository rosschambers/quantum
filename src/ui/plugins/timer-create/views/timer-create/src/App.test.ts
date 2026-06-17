import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';

/*
 * Module-level mock state. Each test resets these via beforeEach so the
 * `vi.mock` factory below sees fresh values. The factory closes over them
 * by reference. Mirrors the power-menu App test pattern.
 */
let mockCallSpy = vi.fn();
let mockSubscribeSpy = vi.fn();

vi.mock('@quantum/client', () => ({
    createClient: () => ({
        call: (...args: unknown[]) => {
            mockCallSpy(...args);
            return Promise.resolve(undefined);
        },
        subscribe: (...args: unknown[]) => {
            mockSubscribeSpy(...args);
            return () => {};
        },
        close: vi.fn(),
    }),
    __esModule: true,
}));

import App from './App.svelte';

beforeEach(() => {
    mockCallSpy = vi.fn();
    mockSubscribeSpy = vi.fn();
});

async function settle(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 10));
    await tick();
    await tick();
}

describe('TimerCreate App', () => {
    it('renders the backdrop and the centered card', () => {
        const { container } = render(App);
        expect(container.querySelector('.backdrop')).not.toBeNull();
        expect(container.querySelector('.card')).not.toBeNull();
    });

    it('Escape key calls view.hide with the bare canonical name', async () => {
        render(App);
        await tick();
        await fireEvent.keyDown(document, { key: 'Escape' });
        await tick();
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/timer-create/timer-create',
        );
        expect(hidden).toBe(true);
    });
});
