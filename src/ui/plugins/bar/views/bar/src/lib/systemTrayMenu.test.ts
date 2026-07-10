import { describe, it, expect, vi } from 'vitest';
import type { SystemTrayMenuNode } from '@quantum/client';
import { menuNodesToItems } from './systemTrayMenu';

/** Build a menu node with sensible defaults, overriding only the fields a test cares about. */
function node(overrides: Partial<SystemTrayMenuNode>): SystemTrayMenuNode {
    return {
        id: 0,
        label: '',
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

describe('menuNodesToItems', () => {
    it('maps a mixed tree to context-menu items', () => {
        const invoke = vi.fn();
        const nodes: SystemTrayMenuNode[] = [
            node({ id: 1, separator: true }),
            node({ id: 2, label: 'Disabled', enabled: false }),
            node({ id: 3, label: 'Check', toggle_state: true, toggle_type: 'checkmark' }),
            node({ id: 4, label: 'Radio', toggle_state: true, toggle_type: 'radio' }),
            node({ id: 5, label: 'Hidden', visible: false }),
            node({
                id: 6,
                label: 'Parent',
                children: [node({ id: 7, label: 'Child' })],
            }),
        ];

        const items = menuNodesToItems('org.example.item', nodes, invoke);

        // Invisible node is omitted, so five of the six nodes survive.
        expect(items).toHaveLength(5);

        const [separator, disabled, checkmark, radio, parent] = items;

        expect(separator).toEqual({ separator: true });
        expect(disabled.disabled).toBe(true);
        expect(checkmark.checked).toBe(true);
        expect(radio.checked).toBe('radio');

        expect(parent.children).toHaveLength(1);
        expect(parent.onSelect).toBeUndefined();

        // A leaf carries an onSelect that invokes with its own node id.
        checkmark.onSelect?.();
        expect(invoke).toHaveBeenCalledWith(3);
    });
});
