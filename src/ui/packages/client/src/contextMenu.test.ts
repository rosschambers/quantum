// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { clampToViewport, openContextMenu, closeContextMenu, type MenuItem } from './contextMenu';

function rightClickAt(x: number, y: number): MouseEvent {
  const target = document.createElement('div');
  document.body.appendChild(target);
  const event = new MouseEvent('contextmenu', { clientX: x, clientY: y, bubbles: true, cancelable: true });
  Object.defineProperty(event, 'target', { value: target });
  return event;
}

function menuRoot(): HTMLElement | null {
  return document.querySelector('[data-quantum-context-menu]');
}

function allMenuRoots(): HTMLElement[] {
  return Array.from(document.querySelectorAll('[data-quantum-context-menu]'));
}

function buttonWithText(scope: ParentNode, text: string): HTMLButtonElement {
  const buttons = Array.from(scope.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));
  const match = buttons.find((button) => button.textContent?.includes(text));
  if (!match) {
    throw new Error(`no menu item with text ${text}`);
  }
  return match;
}

afterEach(() => {
  closeContextMenu();
  document.body.innerHTML = '';
});

describe('clampToViewport', () => {
  it('returns the cursor position when the menu fits', () => {
    expect(clampToViewport(10, 20, 100, 80, 1000, 800)).toEqual({ x: 10, y: 20 });
  });

  it('shifts left when the menu would overflow the right edge', () => {
    expect(clampToViewport(950, 20, 100, 80, 1000, 800)).toEqual({ x: 900, y: 20 });
  });

  it('shifts up when the menu would overflow the bottom edge', () => {
    expect(clampToViewport(10, 760, 100, 80, 1000, 800)).toEqual({ x: 10, y: 720 });
  });

  it('clamps both axes near the bottom-right corner', () => {
    expect(clampToViewport(990, 790, 100, 80, 1000, 800)).toEqual({ x: 900, y: 720 });
  });

  it('never returns a negative coordinate when the menu is larger than the viewport', () => {
    expect(clampToViewport(10, 10, 1200, 900, 1000, 800)).toEqual({ x: 0, y: 0 });
  });
});

describe('openContextMenu', () => {
  it('suppresses the native menu via preventDefault', () => {
    const event = rightClickAt(10, 10);
    const spy = vi.spyOn(event, 'preventDefault');
    openContextMenu(event, [{ label: 'One' }]);
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it('renders a role=menu with one role=menuitem per non-separator item', () => {
    openContextMenu(rightClickAt(10, 10), [
      { label: 'One' },
      { separator: true },
      { label: 'Two' },
    ]);
    const root = menuRoot();
    expect(root).not.toBeNull();
    expect(root!.getAttribute('role')).toBe('menu');
    expect(root!.querySelectorAll('[role="menuitem"]').length).toBe(2);
  });

  it('renders the label via textContent (no markup injection)', () => {
    openContextMenu(rightClickAt(10, 10), [{ label: '<b>x</b>' }]);
    const item = menuRoot()!.querySelector('[role="menuitem"]')!;
    expect(item.querySelector('b')).toBeNull();
    expect(item.textContent).toContain('<b>x</b>');
  });

  it('fires onSelect once and removes the menu when an item is clicked', () => {
    const onSelect = vi.fn();
    openContextMenu(rightClickAt(10, 10), [{ label: 'Go', onSelect }]);
    const item = menuRoot()!.querySelector('[role="menuitem"]') as HTMLButtonElement;
    item.click();
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(menuRoot()).toBeNull();
  });

  it('does not fire onSelect for a disabled item', () => {
    const onSelect = vi.fn();
    openContextMenu(rightClickAt(10, 10), [{ label: 'Nope', disabled: true, onSelect }]);
    const item = menuRoot()!.querySelector('[role="menuitem"]') as HTMLButtonElement;
    item.click();
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('replaces an already-open menu (only one in the DOM)', () => {
    openContextMenu(rightClickAt(10, 10), [{ label: 'First' }]);
    openContextMenu(rightClickAt(20, 20), [{ label: 'Second' }]);
    expect(document.querySelectorAll('[data-quantum-context-menu]').length).toBe(1);
    expect(menuRoot()!.textContent).toContain('Second');
  });
});

describe('dismissal', () => {
  const items: MenuItem[] = [{ label: 'One' }];

  it('closes on Escape', () => {
    openContextMenu(rightClickAt(10, 10), items);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(menuRoot()).toBeNull();
  });

  it('closes on an outside pointerdown', () => {
    openContextMenu(rightClickAt(10, 10), items);
    const outside = document.createElement('div');
    document.body.appendChild(outside);
    outside.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }));
    expect(menuRoot()).toBeNull();
  });

  it('does not close on a pointerdown inside the menu', () => {
    openContextMenu(rightClickAt(10, 10), items);
    menuRoot()!.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }));
    expect(menuRoot()).not.toBeNull();
  });

  it('closes on scroll', () => {
    openContextMenu(rightClickAt(10, 10), items);
    document.dispatchEvent(new Event('scroll'));
    expect(menuRoot()).toBeNull();
  });

  it('closes on window blur', () => {
    openContextMenu(rightClickAt(10, 10), items);
    window.dispatchEvent(new Event('blur'));
    expect(menuRoot()).toBeNull();
  });
});

describe('options', () => {
  it('calls ensureSpace with a pixel extent on open', () => {
    const ensureSpace = vi.fn().mockResolvedValue(undefined);
    openContextMenu(rightClickAt(10, 40), [{ label: 'X' }], { ensureSpace });
    expect(ensureSpace).toHaveBeenCalledTimes(1);
    expect(typeof ensureSpace.mock.calls[0][0]).toBe('number');
  });

  it('calls onClose when the menu is dismissed', () => {
    const onClose = vi.fn();
    openContextMenu(rightClickAt(10, 10), [{ label: 'X' }], { onClose });
    closeContextMenu();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('calls onClose exactly once even when closed twice', () => {
    const onClose = vi.fn();
    openContextMenu(rightClickAt(10, 10), [{ label: 'X' }], { onClose });
    closeContextMenu();
    closeContextMenu();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('keeps the menu hidden until ensureSpace resolves, then reveals it', async () => {
    let resolveSpace: () => void = () => {};
    const ensureSpace = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveSpace = resolve;
        }),
    );
    openContextMenu(rightClickAt(10, 40), [{ label: 'X' }], { ensureSpace });
    const root = menuRoot();
    expect(root).not.toBeNull();
    // Hidden while the surface is still growing, so it cannot paint at the
    // pre-grow position and then jump.
    expect(root!.style.visibility).toBe('hidden');
    resolveSpace();
    await Promise.resolve();
    await Promise.resolve();
    expect(root!.style.visibility).toBe('');
  });

  it('positions immediately when no ensureSpace hook is given', () => {
    openContextMenu(rightClickAt(10, 10), [{ label: 'X' }]);
    expect(menuRoot()!.style.visibility).toBe('');
  });

  it('drops down from the anchor rect bottom-left, ignoring the cursor', () => {
    openContextMenu(rightClickAt(300, 400), [{ label: 'X' }], {
      anchorRect: { x: 50, y: 0, width: 40, height: 30 },
    });
    const root = menuRoot()!;
    expect(root.style.left).toBe('50px');
    expect(root.style.top).toBe('30px');
  });

  it('calls onPlaced exactly once with a numeric rectangle on open', () => {
    const onPlaced = vi.fn();
    openContextMenu(rightClickAt(10, 10), [{ label: 'X' }], { onPlaced });
    expect(onPlaced).toHaveBeenCalledTimes(1);
    const rect = onPlaced.mock.calls[0][0];
    expect(typeof rect.x).toBe('number');
    expect(typeof rect.y).toBe('number');
    expect(typeof rect.width).toBe('number');
    expect(typeof rect.height).toBe('number');
  });

  it('calls onPlaced after the menu is revealed in the ensureSpace path', async () => {
    let resolveSpace: () => void = () => {};
    const ensureSpace = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveSpace = resolve;
        }),
    );
    const onPlaced = vi.fn();
    openContextMenu(rightClickAt(10, 40), [{ label: 'X' }], { ensureSpace, onPlaced });
    expect(onPlaced).not.toHaveBeenCalled();
    resolveSpace();
    await Promise.resolve();
    await Promise.resolve();
    expect(onPlaced).toHaveBeenCalledTimes(1);
    const rect = onPlaced.mock.calls[0][0];
    expect(typeof rect.x).toBe('number');
    expect(typeof rect.width).toBe('number');
  });
});

describe('checked and radio state', () => {
  it('renders a check mark glyph for checked: true', () => {
    openContextMenu(rightClickAt(10, 10), [{ label: 'A', checked: true, onSelect: vi.fn() }]);
    const item = buttonWithText(menuRoot()!, 'A');
    expect(item.textContent).toContain('\u2713');
  });

  it('renders a filled circle glyph for checked: radio', () => {
    openContextMenu(rightClickAt(10, 10), [{ label: 'A', checked: 'radio', onSelect: vi.fn() }]);
    const item = buttonWithText(menuRoot()!, 'A');
    expect(item.textContent).toContain('\u25CF');
  });

  it('lets an explicit icon win over the checked glyph', () => {
    openContextMenu(rightClickAt(10, 10), [{ label: 'A', icon: 'X', checked: true }]);
    const item = buttonWithText(menuRoot()!, 'A');
    expect(item.textContent).toContain('X');
    expect(item.textContent).not.toContain('\u2713');
  });
});

describe('nested submenus', () => {
  it('opens a flyout submenu on mouseenter of a parent item', () => {
    openContextMenu(rightClickAt(10, 10), [
      { label: 'Parent', children: [{ label: 'Child', onSelect: vi.fn() }] },
    ]);
    expect(allMenuRoots().length).toBe(1);
    const parent = buttonWithText(menuRoot()!, 'Parent');
    parent.dispatchEvent(new MouseEvent('mouseenter'));
    const roots = allMenuRoots();
    expect(roots.length).toBe(2);
    const submenu = roots[1];
    expect(buttonWithText(submenu, 'Child')).not.toBeNull();
  });

  it('renders a trailing submenu indicator on a parent item', () => {
    openContextMenu(rightClickAt(10, 10), [
      { label: 'Parent', children: [{ label: 'Child' }] },
    ]);
    const parent = buttonWithText(menuRoot()!, 'Parent');
    expect(parent.textContent).toContain('\u25B8');
  });

  it('does not dismiss when a pointerdown lands inside an open submenu', () => {
    openContextMenu(rightClickAt(10, 10), [
      { label: 'Parent', children: [{ label: 'Child', onSelect: vi.fn() }] },
    ]);
    buttonWithText(menuRoot()!, 'Parent').dispatchEvent(new MouseEvent('mouseenter'));
    expect(allMenuRoots().length).toBe(2);
    const child = buttonWithText(allMenuRoots()[1], 'Child');
    child.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }));
    expect(allMenuRoots().length).toBe(2);
  });

  it('closes the whole tree and fires onSelect when a submenu leaf is clicked', () => {
    const onSelect = vi.fn();
    openContextMenu(rightClickAt(10, 10), [
      { label: 'Parent', children: [{ label: 'Child', onSelect }] },
    ]);
    buttonWithText(menuRoot()!, 'Parent').dispatchEvent(new MouseEvent('mouseenter'));
    const child = buttonWithText(allMenuRoots()[1], 'Child');
    child.click();
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(allMenuRoots().length).toBe(0);
  });

  it('reports a union placement rectangle covering the submenu when it opens', () => {
    const rectSpy = vi
      .spyOn(HTMLElement.prototype, 'getBoundingClientRect')
      .mockImplementation(function (this: HTMLElement) {
        const left = parseFloat(this.style.left) || 0;
        const top = parseFloat(this.style.top) || 0;
        const width = 160;
        const height = 100;
        return {
          x: left,
          y: top,
          width,
          height,
          top,
          left,
          right: left + width,
          bottom: top + height,
          toJSON() {},
        } as DOMRect;
      });
    try {
      const onPlaced = vi.fn();
      openContextMenu(rightClickAt(10, 10), [
        { label: 'Parent', children: [{ label: 'Child', onSelect: vi.fn() }] },
      ], { onPlaced });
      buttonWithText(menuRoot()!, 'Parent').dispatchEvent(new MouseEvent('mouseenter'));
      expect(onPlaced.mock.calls.length).toBeGreaterThanOrEqual(2);
      const submenuLeft = allMenuRoots()[1].getBoundingClientRect().x;
      const lastRect = onPlaced.mock.calls[onPlaced.mock.calls.length - 1][0];
      expect(lastRect.x + lastRect.width).toBeGreaterThanOrEqual(submenuLeft);
    } finally {
      rectSpy.mockRestore();
    }
  });

  it('keeps a simple leaf menu working (no regression)', () => {
    const onSelect = vi.fn();
    openContextMenu(rightClickAt(10, 10), [{ label: 'X', onSelect }]);
    expect(menuRoot()).not.toBeNull();
    buttonWithText(menuRoot()!, 'X').click();
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(allMenuRoots().length).toBe(0);
  });
});
