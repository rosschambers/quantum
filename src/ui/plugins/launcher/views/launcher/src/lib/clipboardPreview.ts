// What the launcher's clipboard-mode preview pane shows for the highlighted
// result. A text entry's primary action already carries its full text (the
// clipboard provider's `copy` action), so the preview shows exactly what Enter
// will copy. No extra IPC.
//
// The clipboard store accepts entries of many megabytes and this runs on every
// highlight change, so sizes and line counts are computed without copying the
// text, and only the first PREVIEW_CHARACTER_LIMIT characters are rendered.
import type { Match } from './types';
import { isThumbnailIcon, resolveIcon } from './icon';

// Rendering more than this many characters in the preview makes the launcher
// sluggish for no benefit: nobody reads that far in a side pane.
export const PREVIEW_CHARACTER_LIMIT = 100_000;

export type ClipboardPreview =
  | { kind: 'text'; text: string; hiddenCharacters: number; details: string }
  | { kind: 'image'; source: string; details: string }
  | { kind: 'other'; details: string };

const KILOBYTE = 1024;
const MEGABYTE = 1024 * 1024;

export function humanSize(bytes: number): string {
  if (bytes === 1) {
    return '1 byte';
  }
  if (bytes < KILOBYTE) {
    return `${bytes} bytes`;
  }
  // Round first, then pick the unit, so 1048575 bytes reads "1.0 MB" rather
  // than "1024.0 KB".
  const kilobytes = Math.round((bytes / KILOBYTE) * 10) / 10;
  if (kilobytes < KILOBYTE) {
    return `${kilobytes.toFixed(1)} KB`;
  }
  return `${(bytes / MEGABYTE).toFixed(1)} MB`;
}

export function lineCount(text: string): number {
  if (text === '') {
    return 0;
  }
  let newlines = 0;
  let index = text.indexOf('\n');
  while (index !== -1) {
    newlines += 1;
    index = text.indexOf('\n', index + 1);
  }
  // One trailing newline ends the last line rather than starting a new one.
  if (text.endsWith('\n')) {
    return newlines;
  }
  return newlines + 1;
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff;
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}

// The UTF-8 encoded length of `text`, counted without encoding it. A lone
// surrogate counts as 3 bytes, the replacement character TextEncoder writes.
export function utf8ByteLength(text: string): number {
  let bytes = 0;
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index);
    if (code < 0x80) {
      bytes += 1;
    } else if (code < 0x800) {
      bytes += 2;
    } else if (isHighSurrogate(code) && isLowSurrogate(text.charCodeAt(index + 1))) {
      bytes += 4;
      index += 1;
    } else {
      bytes += 3;
    }
  }
  return bytes;
}

// The first PREVIEW_CHARACTER_LIMIT characters, never ending on half of a
// surrogate pair.
function previewText(text: string): string {
  if (text.length <= PREVIEW_CHARACTER_LIMIT) {
    return text;
  }
  let end = PREVIEW_CHARACTER_LIMIT;
  if (isHighSurrogate(text.charCodeAt(end - 1))) {
    end -= 1;
  }
  return text.slice(0, end);
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
    const size = humanSize(utf8ByteLength(text));
    const shown = previewText(text);
    return {
      kind: 'text',
      text: shown,
      hiddenCharacters: text.length - shown.length,
      details: joinDetails([match.subtitle, size, `${lines} ${lineLabel}`]),
    };
  }
  const source = isThumbnailIcon(match.icon) ? resolveIcon(match.icon) : undefined;
  if (source) {
    return { kind: 'image', source, details: joinDetails([match.subtitle, match.title]) };
  }
  return { kind: 'other', details: joinDetails([match.subtitle, match.title]) };
}
