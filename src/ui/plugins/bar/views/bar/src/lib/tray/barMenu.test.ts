import { describe, it, expect, vi, afterEach } from 'vitest';
import { closeContextMenu, type MenuItem } from '@quantum/client';
import { barViewName, monitorView, wireBarMenu } from './barMenu';

function contextMenu(): HTMLElement | null {
    return document.querySelector('[data-quantum-context-menu]');
}

function menuItem(text: string): HTMLButtonElement | undefined {
    return Array.from(
        document.querySelectorAll('[data-quantum-context-menu] [role="menuitem"]'),
    ).find((el) => el.textContent?.includes(text)) as HTMLButtonElement | undefined;
}

function makeMockClient() {
    const call = vi.fn(() => Promise.resolve(undefined));
    const client = { call, subscribe: vi.fn(), close: vi.fn() };
    return { client, call };
}

afterEach(() => {
    closeContextMenu();
    (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = undefined;
    document.body.innerHTML = '';
});

describe('barViewName / monitorView', () => {
    it('returns bare names with no monitor set', () => {
        expect(barViewName()).toBe('plugin/bar/bar');
        expect(monitorView('widgets/power-menu')).toBe('widgets/power-menu');
    });

    it('appends the @monitor suffix when set', () => {
        (window as unknown as { __quantum_monitor?: string }).__quantum_monitor = 'DP-1';
        expect(barViewName()).toBe('plugin/bar/bar@DP-1');
        expect(monitorView('widgets/power-menu')).toBe('widgets/power-menu@DP-1');
    });
});

describe('wireBarMenu', () => {
    function setup(items: MenuItem[]) {
        const node = document.createElement('button');
        document.body.appendChild(node);
        const { client, call } = makeMockClient();
        const teardown = wireBarMenu(node, client as never, () => items);
        return { node, call, teardown };
    }

    it('opens a menu with the built items on right-click', async () => {
        const onSelect = vi.fn();
        const { node } = setup([{ label: 'Do thing', onSelect }]);

        expect(contextMenu()).toBeNull();
        node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));

        expect(contextMenu()).not.toBeNull();
        expect(menuItem('Do thing')).toBeTruthy();
    });

    function setupWithTrigger(
        items: MenuItem[],
        trigger: 'click' | 'contextmenu',
    ) {
        const node = document.createElement('button');
        document.body.appendChild(node);
        const { client, call } = makeMockClient();
        const teardown = wireBarMenu(node, client as never, () => items, trigger);
        return { node, call, teardown };
    }

    it('opens on a left-click when trigger is "click"', () => {
        const { node } = setupWithTrigger([{ label: 'Do thing' }], 'click');

        expect(contextMenu()).toBeNull();
        node.dispatchEvent(new MouseEvent('click', { bubbles: true }));

        expect(contextMenu()).not.toBeNull();
        expect(menuItem('Do thing')).toBeTruthy();
    });

    it('ignores right-click when trigger is "click"', () => {
        const { node } = setupWithTrigger([{ label: 'Do thing' }], 'click');

        node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));

        expect(contextMenu()).toBeNull();
    });

    it('ignores a left-click with the default contextmenu trigger', () => {
        const { node } = setup([{ label: 'Do thing' }]);

        node.dispatchEvent(new MouseEvent('click', { bubbles: true }));

        expect(contextMenu()).toBeNull();
    });

    it('removes the click listener on teardown when trigger is "click"', () => {
        const { node, teardown } = setupWithTrigger([{ label: 'X' }], 'click');
        teardown();
        node.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        expect(contextMenu()).toBeNull();
    });

    it('expands the input region on open and resets it on close', () => {
        const { node, call } = setup([{ label: 'X' }]);
        node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));

        const openCall = call.mock.calls.find(
            (args) =>
                args[0] === 'view.set_input_region' &&
                (args[1] as { region: unknown }).region !== null,
        );
        expect(openCall).toBeTruthy();
        const region = (openCall![1] as {
            region: { x: number; y: number; width: number; height: number };
        }).region;
        expect(typeof region.x).toBe('number');
        expect(typeof region.width).toBe('number');

        call.mockClear();
        closeContextMenu();

        expect(call).toHaveBeenCalledWith('view.set_input_region', {
            name: 'plugin/bar/bar',
            region: null,
        });
    });

    it('removes the listener on teardown', () => {
        const { node, teardown } = setup([{ label: 'X' }]);
        teardown();
        node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
        expect(contextMenu()).toBeNull();
    });

    it('opens no menu and touches no input region when there are no items', () => {
        const { node, call } = setup([]);
        node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));

        expect(contextMenu()).toBeNull();
        expect(call).not.toHaveBeenCalledWith(
            'view.set_input_region',
            expect.anything(),
        );
    });

    it('drops leading, trailing, and consecutive separators', () => {
        const { node } = setup([
            { separator: true, label: '' },
            { label: 'A' },
            { separator: true, label: '' },
            { separator: true, label: '' },
            { label: 'B' },
            { separator: true, label: '' },
        ]);
        node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));

        const menu = contextMenu();
        expect(menu).not.toBeNull();
        expect(menuItem('A')).toBeTruthy();
        expect(menuItem('B')).toBeTruthy();
        // One divider survives, between A and B.
        expect(menu!.querySelectorAll('hr')).toHaveLength(1);
    });

    function setupAsync(builder: () => Promise<MenuItem[]>) {
        const node = document.createElement('button');
        document.body.appendChild(node);
        const { client, call } = makeMockClient();
        const teardown = wireBarMenu(node, client as never, builder);
        return { node, call, teardown };
    }

    async function flushMicrotasks(): Promise<void> {
        await Promise.resolve();
        await Promise.resolve();
    }

    it('opens a menu with items resolved from an async builder', async () => {
        const onSelect = vi.fn();
        const { node } = setupAsync(() =>
            Promise.resolve([{ label: 'Async thing', onSelect }]),
        );

        node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
        expect(contextMenu()).toBeNull();

        await flushMicrotasks();

        expect(contextMenu()).not.toBeNull();
        expect(menuItem('Async thing')).toBeTruthy();
    });

    it('opens no menu and touches no input region when async builder resolves empty', async () => {
        const { node, call } = setupAsync(() => Promise.resolve([]));

        node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
        await flushMicrotasks();

        expect(contextMenu()).toBeNull();
        expect(call).not.toHaveBeenCalledWith(
            'view.set_input_region',
            expect.anything(),
        );
    });

    it('closes an open menu and resets the region on teardown', () => {
        const { node, call, teardown } = setup([{ label: 'X' }]);
        node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
        expect(contextMenu()).not.toBeNull();

        call.mockClear();
        teardown();

        expect(contextMenu()).toBeNull();
        expect(call).toHaveBeenCalledWith('view.set_input_region', {
            name: 'plugin/bar/bar',
            region: null,
        });
    });
});
