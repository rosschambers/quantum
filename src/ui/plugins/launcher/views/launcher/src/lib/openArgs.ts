// The launcher can be opened with arguments, for example the Super+V bind's
// `quantumctl show launcher --args '{"query":";"}'`. The host injects them as
// `window.__quantum_args` on every open (`null` when an open carries none), so
// reading them each time the launcher is shown is always current. The files
// and file-viewer views read the same global.

export function initialQueryFrom(args: unknown): string {
  if (typeof args !== 'object' || args === null) {
    return '';
  }
  const query = (args as { query?: unknown }).query;
  if (typeof query === 'string') {
    return query;
  }
  return '';
}

export function currentInitialQuery(): string {
  return initialQueryFrom((window as { __quantum_args?: unknown }).__quantum_args);
}
