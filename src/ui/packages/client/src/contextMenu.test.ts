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
});
