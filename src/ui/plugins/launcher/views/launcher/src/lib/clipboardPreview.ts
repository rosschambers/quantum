// What the launcher's clipboard-mode preview pane shows for the highlighted
// result. A text entry's primary action already carries its full text (the
// clipboard provider's `copy` action), so the preview shows exactly what Enter
// will copy. No extra IPC.
import type { Match } from './types';
import { isThumbnailIcon, resolveIcon } from './icon';

export type ClipboardPreview =
  | { kind: 'text'; text: string; details: string }
  | { kind: 'image'; source: string; details: string }
  | { kind: 'other'; details: string };

export function humanSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} bytes`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function lineCount(text: string): number {
  if (text === '') {
    return 0;
  }
  return text.replace(/\n$/, '').split('\n').length;
}

function copyText(match: Match): string | undefined {
  if (match.action.kind !== 'copy') {
    return undefined;
  }
  const data = match.action.data as { text?: unknown } | null | undefined;
  if (data !== null && data !== undefined && typeof data.text === 'string') {
    return data.text;
  }
  return undefined;
}

function joinDetails(parts: (string | undefined)[]): string {
  return parts.filter((part): part is string => Boolean(part)).join(' · ');
}

export function clipboardPreview(match: Match | undefined): ClipboardPreview | null {
  if (!match || match.provider !== 'clipboard') {
    return null;
  }
  const text = copyText(match);
  if (text !== undefined) {
    const lines = lineCount(text);
    const lineLabel = lines === 1 ? 'line' : 'lines';
    const size = humanSize(new TextEncoder().encode(text).length);
    return { kind: 'text', text, details: joinDetails([match.subtitle, size, `${lines} ${lineLabel}`]) };
  }
  const source = isThumbnailIcon(match.icon) ? resolveIcon(match.icon) : undefined;
  if (source) {
    return { kind: 'image', source, details: joinDetails([match.subtitle, match.title]) };
  }
  return { kind: 'other', details: joinDetails([match.subtitle, match.title]) };
}
