export interface IconRef {
  kind: string;
  data: string;
}

export function isIconRef(value: unknown): value is IconRef {
  return (
    typeof value === 'object' &&
    value !== null &&
    'kind' in value &&
    'data' in value
  );
}

// A launcher action envelope. `kind` selects the variant; `data` carries its
// payload. Known kinds include `launch`, `shell`, `focus`, `custom`, and
// `copy` (`{ kind: 'copy', data: { text: string } }`), which copies text to
// the clipboard without launching anything.
export interface ActionRef {
  kind: string;
  data: unknown;
}

// A secondary action a provider attaches to a result, surfaced in the
// launcher's Ctrl+K / right-click actions panel. `danger` renders the entry in
// the error color; `icon` is an optional glyph string.
export interface MenuAction {
  label: string;
  icon?: string;
  danger?: boolean;
  action: ActionRef;
}

export interface Match {
  id: string;
  provider: string;
  title: string;
  subtitle?: string;
  icon?: string | IconRef;
  score: number;
  action: ActionRef;
  actions?: MenuAction[];
}
