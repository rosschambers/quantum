// Pure mapper from the system tray's menu tree (`SystemTrayMenuNode[]`) to the
// shared context-menu item shape (`MenuItem[]`). Kept as a standalone module so
// it is trivially unit-testable without rendering the component or wiring a
// client: the caller supplies the `invoke` callback that a leaf's selection
// fires.

import type { MenuItem, SystemTrayMenuNode } from '@quantum/client';

/** The `checked` state a node projects onto its menu item, if any. */
function checkedState(node: SystemTrayMenuNode): boolean | 'radio' | undefined {
    if (node.toggle_state !== true) {
        return undefined;
    }
    return node.toggle_type === 'radio' ? 'radio' : true;
}

/**
 * Map a system tray menu tree to context-menu items.
 *
 * - Nodes with `visible === false` are dropped entirely.
 * - A `separator` node becomes a bare divider.
 * - A node with children is a submenu parent: it carries `children` and no
 *   `onSelect` (opening the flyout is its action).
 * - A leaf carries an `onSelect` that fires `invoke(node.id)`.
 */
export function menuNodesToItems(
    service: string,
    nodes: SystemTrayMenuNode[],
    invoke: (itemId: number) => void,
): MenuItem[] {
    const items: MenuItem[] = [];
    for (const node of nodes) {
        if (node.visible === false) {
            continue;
        }
        if (node.separator === true) {
            items.push({ separator: true } as MenuItem);
            continue;
        }
        const hasChildren = node.children.length > 0;
        items.push({
            label: node.label,
            disabled: node.enabled === false ? true : undefined,
            checked: checkedState(node),
            children: hasChildren
                ? menuNodesToItems(service, node.children, invoke)
                : undefined,
            onSelect: hasChildren ? undefined : () => invoke(node.id),
        });
    }
    return items;
}
