import { describe, it, expect, vi, afterEach } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import { tick, flushSync } from 'svelte';
import { closeContextMenu } from '@quantum/client';
import type { SystemTrayItem, SystemTrayMenuNode, SystemTrayState } from '@quantum/client';
import SystemTrayItems from './SystemTrayItems.svelte';

function menuNode(overrides: Partial<SystemTrayMenuNode>): SystemTrayMenuNode {
    return {
        id: 1,
        label: 'Open',
        enabled: true,
        visible: true,
        separator: false,
        toggle_type: null,
        toggle_state: null,
        icon_name: null,
        children: [],
        ...overrides,
    };
}

function trayItem(overrides: Partial<SystemTrayItem>): SystemTrayItem {
    return {
        service: 'org.example.item',
        title: 'Example',
        tooltip: '',
        status: 'Active',
        icon: null,
        item_is_menu: false,
        menu: [],
        ...overrides,
    };
}

function mockClient(): { client: any; emit: (state: SystemTrayState) => void } {
    let saved: ((payload: unknown) => void) | undefined;
    const call = vi.fn((method: string) => {
        if (method === 'provider.query') return Promise.resolve({ items: [] });
        return Promise.resolve(undefined);
    });
    const subscribe = vi.fn((_channel: string, callback: (payload: unknown) => void) => {
        saved = callback;
        return () => {};
    });
    const client = { call, subscribe, close: vi.fn() };
    return {
        client,
        emit: (state: SystemTrayState) => saved?.(state),
    };
}

afterEach(() => {
    closeContextMenu();
    document.body.innerHTML = '';
});

describe('SystemTrayItems', () => {
    it('renders a button per item and reserves no space when the tray empties', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemTrayItems, { props: { client } });
        await tick();

        // An empty tray renders nothing at all.
        expect(container.querySelectorAll('.bar-button')).toHaveLength(0);

        emit({
            items: [
                trayItem({ service: 'a', title: 'Alpha' }),
                trayItem({ service: 'b', title: 'Beta' }),
            ],
        });
        flushSync();
        await tick();

        const buttons = container.querySelectorAll('.bar-button');
        expect(buttons).toHaveLength(2);
        expect(buttons[0].getAttribute('title')).toBe('Alpha');
        expect(buttons[1].getAttribute('title')).toBe('Beta');

        emit({ items: [] });
        flushSync();
        await tick();
        expect(container.querySelectorAll('.bar-button')).toHaveLength(0);
    });

    it('renders a text fallback when an item has no loadable icon', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemTrayItems, { props: { client } });
        await tick();

        emit({ items: [trayItem({ service: 'z', title: 'zoom', icon: null })] });
        flushSync();
        await tick();

        const button = container.querySelector('.bar-button');
        expect(button).not.toBeNull();
        expect(button!.querySelector('img')).toBeNull();
        expect(button!.textContent?.trim()).toBe('Z');
    });

    // Regression for I1: a broadcast that only updates an item's fields (a
    // NewTitle/NewToolTip/NewStatus, or a LayoutUpdated on the very item being
    // viewed) must NOT tear down that item's menu wiring and slam an open
    // context menu shut. Before the fix a single shared $effect that read
    // `state.items` re-ran on every broadcast, and its teardown called
    // `wireBarMenu`'s cleanup, which closes the context menu unconditionally.
    it('keeps an open context menu open when another tray update arrives', async () => {
        const { client, emit } = mockClient();
        const { container } = render(SystemTrayItems, { props: { client } });
        await tick();

        emit({
            items: [trayItem({ service: 'a', title: 'Alpha', menu: [menuNode({})] })],
        });
        flushSync();
        await tick();

        const button = container.querySelector('.bar-button') as HTMLElement;
        expect(button).not.toBeNull();

        // Open the item's context menu. The builder is asynchronous
        // (about_to_show then provider.query), so wait for the menu to appear.
        button.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
        await vi.waitFor(() => {
            expect(document.querySelector('[data-quantum-context-menu]')).not.toBeNull();
        });

        // A new broadcast updates the SAME item (same service key) with a fresh
        // title. This is the churn that used to close the menu.
        emit({
            items: [trayItem({ service: 'a', title: 'Alpha renamed', menu: [menuNode({})] })],
        });
        flushSync();
        await tick();

        expect(document.querySelector('[data-quantum-context-menu]')).not.toBeNull();
    });
});
