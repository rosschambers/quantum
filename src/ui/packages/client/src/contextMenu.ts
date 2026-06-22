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
}

interface ActiveMenu {
  root: HTMLElement;
  cleanup: () => void;
}

let active: ActiveMenu | null = null;

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

function buildItem(doc: Document, item: MenuItem): HTMLElement {
  if (item.separator) {
    return doc.createElement('hr');
  }
  const button = doc.createElement('button');
  button.type = 'button';
  button.setAttribute('role', 'menuitem');
  if (item.icon) {
    const icon = doc.createElement('span');
    icon.setAttribute('aria-hidden', 'true');
    icon.textContent = item.icon;
    button.appendChild(icon);
  }
  const label = doc.createElement('span');
  label.textContent = item.label;
  button.appendChild(label);
  if (item.danger) {
    button.dataset.danger = 'true';
  }
  if (item.disabled) {
    button.disabled = true;
    button.setAttribute('aria-disabled', 'true');
  } else {
    button.addEventListener('click', () => {
      closeContextMenu();
      item.onSelect?.();
    });
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

  const root = doc.createElement('div');
  root.setAttribute('data-quantum-context-menu', '');
  root.setAttribute('role', 'menu');
  for (const item of items) {
    root.appendChild(buildItem(doc, item));
  }
  doc.body.appendChild(root);

  if (options?.ensureSpace) {
    // The surface must grow before the menu can be placed: positioning now
    // would clamp it against the pre-grow viewport (a thin bar forces it to
    // the top), then it would visibly jump once the surface grows. Keep it
    // hidden until the grow resolves, then position and reveal in one paint.
    root.style.visibility = 'hidden';
    const needed = Math.ceil(event.clientY + root.getBoundingClientRect().height);
    const reveal = (): void => {
      if (active?.root !== root) {
        return;
      }
      position(doc, root, event.clientX, event.clientY);
      root.style.visibility = '';
    };
    options.ensureSpace(needed).then(reveal).catch(reveal);
  } else {
    position(doc, root, event.clientX, event.clientY);
  }

  const onKeyDown = (keyEvent: KeyboardEvent): void => {
    if (keyEvent.key === 'Escape') {
      closeContextMenu();
    }
  };
  const onPointerDown = (pointerEvent: Event): void => {
    if (!root.contains(pointerEvent.target as Node)) {
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
    root,
    cleanup: () => {
      view.removeEventListener('keydown', onKeyDown, true);
      view.removeEventListener('blur', onDismiss);
      doc.removeEventListener('pointerdown', onPointerDown, true);
      doc.removeEventListener('scroll', onDismiss, true);
      doc.removeEventListener('visibilitychange', onVisibility);
      root.remove();
      options?.onClose?.();
    },
  };
}
