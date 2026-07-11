// A small, framework-free right-click context menu shared by every Quantum
// view. First-party Svelte views import `openContextMenu` from `@quantum/client`;
// the same runtime is injected onto `window.quantum.openContextMenu` for plain
// third-party plugin pages. The native WebKit menu is already suppressed
// host-side, so calling this from a DOM `contextmenu` handler is all a view
// needs. Menu actions are plain callbacks, so the view decides whether to call
// the host (`client.call(...)`) or do something local.

export interface MenuItem {
  /** Visible text. Rendered via textContent only, never as markup. */
  label: string;
  /** Optional leading glyph or short text; also rendered as plain text. */
  icon?: string;
  /** When true the item is shown greyed out and cannot be selected. */
  disabled?: boolean;
  /** When true the item is styled as destructive (for example "Forget"). */
  danger?: boolean;
  /** When true this entry is a divider; all other fields are ignored. */
  separator?: boolean;
  /**
   * Nested items. When present and non-empty this item becomes a submenu
   * parent: it renders a trailing indicator, opens a flyout on hover/click,
   * and does not fire `onSelect` (opening the flyout is its action).
   */
  children?: MenuItem[];
  /**
   * Renders a leading state glyph in the icon slot when the item has no
   * explicit `icon` (an explicit `icon` always wins). `true` shows a check
   * mark; `'radio'` shows a filled circle. Purely visual; the caller owns the
   * underlying state and rebuilds items to reflect changes.
   */
  checked?: boolean | 'radio';
  /** Invoked once when the item is selected, after the menu closes. */
  onSelect?: () => void;
}

export interface MenuOptions {
  /**
   * Optional hook for surfaces that must grow before a downward menu is
   * visible (the bar is a thin strip). Receives the pixel extent the menu
   * needs from the top of the surface and resolves once the surface has grown.
   */
  ensureSpace?: (neededPixels: number) => Promise<void>;
  /**
   * Called once when the menu closes (selection, dismissal, or replacement).
   * Surfaces that grew via `ensureSpace` use this to shrink back.
   */
  onClose?: () => void;
  /**
   * Called after the menu has been positioned, with the bounding rectangle
   * (rounded to integers) of the UNION of every currently-open menu root.
   * Surfaces that gate pointer input by region (the bar) use this to expand
   * their input region to cover the menu so it is clickable. May be called
   * multiple times for one menu: once on open, then again every time a flyout
   * submenu opens or closes and the union rectangle changes.
   */
  onPlaced?: (rect: { x: number; y: number; width: number; height: number }) => void;
  /**
   * Optional anchor rectangle (for example a triggering button's bounding
   * box). When given, the menu drops down from the anchor's bottom-left edge
   * instead of the cursor position, giving a true dropdown for toolbar/bar
   * buttons. Still clamped to the viewport. Omit for cursor placement (the
   * default for right-click menus on roomy surfaces).
   */
  anchorRect?: { x: number; y: number; width: number; height: number };
}

interface ActiveMenu {
  /**
   * Every open root: the top-level menu at index 0, plus one entry per open
   * flyout submenu. Dismissal treats a pointerdown inside ANY of these as
   * "inside"; cleanup removes them all.
   */
  roots: HTMLElement[];
  /** Maps each open parent item button to the submenu root it opened. */
  submenusByParent: Map<HTMLElement, HTMLElement>;
  options?: MenuOptions;
  cleanup: () => void;
}

let active: ActiveMenu | null = null;

const CHECK_GLYPH = '\u2713';
const RADIO_GLYPH = '\u25CF';
const SUBMENU_GLYPH = '\u25B8';

const STYLE_ELEMENT_ID = 'quantum-context-menu-style';
const MENU_STYLES = `
[data-quantum-context-menu] {
  position: fixed;
  z-index: 2147483647;
  display: flex;
  flex-direction: column;
  min-width: 160px;
  padding: 4px;
  margin: 0;
  border-radius: var(--radius-lg, 8px);
  background: var(--color-surface, hsla(230, 14%, 22%, 0.98));
  border: 1px solid var(--color-border, rgba(255, 255, 255, 0.08));
  box-shadow: 0 6px 18px var(--color-shadow, rgba(0, 0, 0, 0.45));
  font-family: var(--font-sans, system-ui, sans-serif);
  font-size: var(--font-size-sm, 12px);
  color: var(--color-fg-alt, #a6adc8);
}
[data-quantum-context-menu] button {
  display: flex;
  align-items: center;
  gap: var(--space-2, 0.5rem);
  width: 100%;
  text-align: left;
  background: transparent;
  border: none;
  border-radius: var(--radius-md, 4px);
  padding: 6px 10px;
  color: inherit;
  font: inherit;
  line-height: 1.2;
  cursor: pointer;
}
[data-quantum-context-menu] button:hover:not(:disabled),
[data-quantum-context-menu] button:focus-visible:not(:disabled) {
  background: var(--color-surface-hover, hsla(230, 14%, 42%, 1));
  color: var(--color-fg, #cdd6f4);
  outline: none;
}
[data-quantum-context-menu] button[data-danger="true"] {
  color: var(--color-warning, #f38ba8);
}
[data-quantum-context-menu] button:disabled {
  opacity: 0.5;
  cursor: default;
}
[data-quantum-context-menu] hr {
  height: 1px;
  margin: 4px 6px;
  border: none;
  background: var(--color-divider, rgba(255, 255, 255, 0.12));
}
[data-quantum-context-menu] button > span:first-child[data-icon-slot="true"] {
  display: inline-flex;
  justify-content: center;
  min-width: var(--space-3, 0.75rem);
}
[data-quantum-context-menu] button > span[data-submenu-indicator="true"] {
  margin-left: auto;
  padding-left: var(--space-2, 0.5rem);
  color: var(--color-fg-alt, #a6adc8);
}
`;

/**
 * Clamp a menu rectangle so it stays fully inside the viewport. Pure math so it
 * can be unit-tested without a layout engine.
 */
export function clampToViewport(
  x: number,
  y: number,
  menuWidth: number,
  menuHeight: number,
  viewportWidth: number,
  viewportHeight: number,
): { x: number; y: number } {
  let clampedX = x;
  let clampedY = y;
  if (clampedX + menuWidth > viewportWidth) {
    clampedX = viewportWidth - menuWidth;
  }
  if (clampedY + menuHeight > viewportHeight) {
    clampedY = viewportHeight - menuHeight;
  }
  if (clampedX < 0) {
    clampedX = 0;
  }
  if (clampedY < 0) {
    clampedY = 0;
  }
  return { x: clampedX, y: clampedY };
}

/** Close the open menu, if any, and remove all of its listeners. */
export function closeContextMenu(): void {
  if (active) {
    active.cleanup();
    active = null;
  }
}

function ensureStyleSheet(doc: Document): void {
  if (doc.getElementById(STYLE_ELEMENT_ID)) {
    return;
  }
  const style = doc.createElement('style');
  style.id = STYLE_ELEMENT_ID;
  style.textContent = MENU_STYLES;
  (doc.head ?? doc.documentElement).appendChild(style);
}

/**
 * The glyph shown in the leading icon slot. An explicit `icon` always wins;
 * otherwise a `checked` state supplies a check mark or a filled circle.
 */
function resolveIconGlyph(item: MenuItem): string | undefined {
  if (item.icon) {
    return item.icon;
  }
  if (item.checked === 'radio') {
    return RADIO_GLYPH;
  }
  if (item.checked) {
    return CHECK_GLYPH;
  }
  return undefined;
}

/**
 * Render a single item's DOM (separator, or a menu button with optional icon
 * slot, label, and submenu indicator). Pure: it wires no event listeners, so
 * `buildMenuInto` owns selection and submenu behaviour.
 */
function renderItemElement(doc: Document, item: MenuItem): HTMLElement {
  if (item.separator) {
    return doc.createElement('hr');
  }
  const button = doc.createElement('button');
  button.type = 'button';
  button.setAttribute('role', 'menuitem');
  const iconGlyph = resolveIconGlyph(item);
  if (iconGlyph) {
    const icon = doc.createElement('span');
    icon.setAttribute('aria-hidden', 'true');
    icon.dataset.iconSlot = 'true';
    icon.textContent = iconGlyph;
    button.appendChild(icon);
  }
  const label = doc.createElement('span');
  label.textContent = item.label;
  button.appendChild(label);
  if (item.children && item.children.length > 0) {
    const indicator = doc.createElement('span');
    indicator.setAttribute('aria-hidden', 'true');
    indicator.dataset.submenuIndicator = 'true';
    indicator.textContent = SUBMENU_GLYPH;
    button.appendChild(indicator);
  }
  if (item.danger) {
    button.dataset.danger = 'true';
  }
  if (item.disabled) {
    button.disabled = true;
    button.setAttribute('aria-disabled', 'true');
  }
  return button;
}

function position(doc: Document, root: HTMLElement, clientX: number, clientY: number): void {
  const view = doc.defaultView;
  const rect = root.getBoundingClientRect();
  const viewportWidth = view?.innerWidth ?? rect.width;
  const viewportHeight = view?.innerHeight ?? rect.height;
  const { x, y } = clampToViewport(clientX, clientY, rect.width, rect.height, viewportWidth, viewportHeight);
  root.style.left = `${x}px`;
  root.style.top = `${y}px`;
}

/**
 * Open a context menu at the event's cursor position. Call from a DOM
 * `contextmenu` handler; this calls `preventDefault` for you.
 */
export function openContextMenu(event: MouseEvent, items: MenuItem[], options?: MenuOptions): void {
  event.preventDefault();
  event.stopPropagation();
  closeContextMenu();

  const targetNode = event.target as Node | null;
  const doc = targetNode?.ownerDocument ?? document;
  const view = doc.defaultView ?? window;

  ensureStyleSheet(doc);

  const roots: HTMLElement[] = [];
  const submenusByParent = new Map<HTMLElement, HTMLElement>();

  // Report the union of every open root's rectangle so a region-gated surface
  // (the bar) covers all flyouts. Called on open and on every submenu change.
  const reportUnionPlacement = (): void => {
    if (!options?.onPlaced) {
      return;
    }
    const visible = roots.filter((candidate) => candidate.style.visibility !== 'hidden');
    if (visible.length === 0) {
      return;
    }
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const candidate of visible) {
      const rect = candidate.getBoundingClientRect();
      minX = Math.min(minX, rect.x);
      minY = Math.min(minY, rect.y);
      maxX = Math.max(maxX, rect.x + rect.width);
      maxY = Math.max(maxY, rect.y + rect.height);
    }
    options.onPlaced({
      x: Math.round(minX),
      y: Math.round(minY),
      width: Math.round(maxX - minX),
      height: Math.round(maxY - minY),
    });
  };

  // Close a parent item's flyout and, recursively, any flyouts opened from
  // within it, then drop them all from the tracked roots.
  const closeSubmenuForItem = (parentButton: HTMLElement): void => {
    const submenu = submenusByParent.get(parentButton);
    if (!submenu) {
      return;
    }
    for (const nestedParent of [...submenusByParent.keys()]) {
      if (nestedParent.parentElement === submenu) {
        closeSubmenuForItem(nestedParent);
      }
    }
    submenusByParent.delete(parentButton);
    const index = roots.indexOf(submenu);
    if (index >= 0) {
      roots.splice(index, 1);
    }
    submenu.remove();
  };

  // Open a flyout for a parent item at its top-right corner, clamped to the
  // viewport. No-op if that item's flyout is already open.
  const openSubmenuForItem = (parentButton: HTMLElement, children: MenuItem[]): void => {
    if (submenusByParent.has(parentButton)) {
      return;
    }
    const submenu = doc.createElement('div');
    submenu.setAttribute('data-quantum-context-menu', '');
    submenu.setAttribute('role', 'menu');
    buildMenuInto(submenu, children);
    doc.body.appendChild(submenu);
    const anchor = parentButton.getBoundingClientRect();
    position(doc, submenu, anchor.x + anchor.width, anchor.y);
    submenusByParent.set(parentButton, submenu);
    roots.push(submenu);
  };

  // Entering an item closes any sibling flyout in the same root (one open path
  // per level) and opens this item's flyout when it has children.
  const enterItem = (parentRoot: HTMLElement, button: HTMLElement, item: MenuItem): void => {
    for (const openParent of [...submenusByParent.keys()]) {
      if (openParent.parentElement === parentRoot && openParent !== button) {
        closeSubmenuForItem(openParent);
      }
    }
    if (item.children && item.children.length > 0) {
      openSubmenuForItem(button, item.children);
    }
    reportUnionPlacement();
  };

  // Render items into a root and wire hover/selection. Shared by the top-level
  // menu and every flyout, so submenus nest recursively.
  function buildMenuInto(parentRoot: HTMLElement, menuItems: MenuItem[]): void {
    for (const item of menuItems) {
      const element = renderItemElement(doc, item);
      parentRoot.appendChild(element);
      if (item.separator || item.disabled) {
        continue;
      }
      const button = element as HTMLButtonElement;
      const hasChildren = !!(item.children && item.children.length > 0);
      button.addEventListener('mouseenter', () => enterItem(parentRoot, button, item));
      button.addEventListener('click', () => {
        if (hasChildren) {
          enterItem(parentRoot, button, item);
          return;
        }
        closeContextMenu();
        item.onSelect?.();
      });
    }
  }

  const root = doc.createElement('div');
  root.setAttribute('data-quantum-context-menu', '');
  root.setAttribute('role', 'menu');
  roots.push(root);
  buildMenuInto(root, items);
  doc.body.appendChild(root);

  // Placement origin: a bar/toolbar button passes its anchor rectangle so the
  // menu drops down from just below the button; otherwise the cursor is used.
  const originX = options?.anchorRect ? options.anchorRect.x : event.clientX;
  const originY = options?.anchorRect
    ? options.anchorRect.y + options.anchorRect.height
    : event.clientY;

  if (options?.ensureSpace) {
    // The surface must grow before the menu can be placed: positioning now
    // would clamp it against the pre-grow viewport (a thin bar forces it to
    // the top), then it would visibly jump once the surface grows. Keep it
    // hidden until the grow resolves, then position and reveal in one paint.
    root.style.visibility = 'hidden';
    const needed = Math.ceil(originY + root.getBoundingClientRect().height);
    const reveal = (): void => {
      if (active?.roots[0] !== root) {
        return;
      }
      position(doc, root, originX, originY);
      root.style.visibility = '';
      reportUnionPlacement();
    };
    options.ensureSpace(needed).then(reveal).catch(reveal);
  } else {
    position(doc, root, originX, originY);
    reportUnionPlacement();
  }

  const onKeyDown = (keyEvent: KeyboardEvent): void => {
    if (keyEvent.key === 'Escape') {
      closeContextMenu();
    }
  };
  const onPointerDown = (pointerEvent: Event): void => {
    const target = pointerEvent.target as Node;
    if (!roots.some((candidate) => candidate.contains(target))) {
      closeContextMenu();
    }
  };
  const onDismiss = (): void => closeContextMenu();
  const onVisibility = (): void => {
    if (doc.hidden) {
      closeContextMenu();
    }
  };

  view.addEventListener('keydown', onKeyDown, true);
  view.addEventListener('blur', onDismiss);
  doc.addEventListener('pointerdown', onPointerDown, true);
  doc.addEventListener('scroll', onDismiss, true);
  doc.addEventListener('visibilitychange', onVisibility);

  active = {
    roots,
    submenusByParent,
    options,
    cleanup: () => {
      view.removeEventListener('keydown', onKeyDown, true);
      view.removeEventListener('blur', onDismiss);
      doc.removeEventListener('pointerdown', onPointerDown, true);
      doc.removeEventListener('scroll', onDismiss, true);
      doc.removeEventListener('visibilitychange', onVisibility);
      for (const candidate of roots) {
        candidate.remove();
      }
      roots.length = 0;
      submenusByParent.clear();
      options?.onClose?.();
    },
  };
}
