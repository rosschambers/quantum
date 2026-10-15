import { describe, expect, it } from 'vitest';
import {
  PREVIEW_CHARACTER_LIMIT,
  clipboardPreview,
  humanSize,
  lineCount,
  utf8ByteLength,
} from './clipboardPreview';
import type { Match } from './types';

function textMatch(text: string): Match {
  return {
    id: '1',
    provider: 'clipboard',
    title: text.split(/\s+/).join(' '),
    subtitle: '5m ago',
    score: 1,
    action: { kind: 'copy', data: { text } },
  };
}

describe('humanSize', () => {
  it('formats bytes, kilobytes and megabytes', () => {
    expect(humanSize(512)).toBe('512 bytes');
    expect(humanSize(2048)).toBe('2.0 KB');
    expect(humanSize(3145728)).toBe('3.0 MB');
  });

  it('says byte for exactly one byte', () => {
    expect(humanSize(1)).toBe('1 byte');
  });

  it('switches unit at the boundaries without showing 1024.0 KB', () => {
    expect(humanSize(1023)).toBe('1023 bytes');
    expect(humanSize(1024)).toBe('1.0 KB');
    expect(humanSize(1048575)).toBe('1.0 MB');
    expect(humanSize(1048576)).toBe('1.0 MB');
  });
});

describe('lineCount', () => {
  it('counts lines and ignores one trailing newline', () => {
    expect(lineCount('')).toBe(0);
    expect(lineCount('a')).toBe(1);
    expect(lineCount('a\nb')).toBe(2);
    expect(lineCount('a\nb\n')).toBe(2);
  });

  it('counts only one trailing newline as ignorable', () => {
    expect(lineCount('\n')).toBe(1);
    expect(lineCount('a\n\n')).toBe(2);
  });
});

describe('utf8ByteLength', () => {
  it('counts one, two, three and four byte characters', () => {
    expect(utf8ByteLength('a')).toBe(1);
    expect(utf8ByteLength('\u00e9')).toBe(2);
    expect(utf8ByteLength('\u20ac')).toBe(3);
    expect(utf8ByteLength('\u{1F600}')).toBe(4);
  });

  it('matches TextEncoder for mixed text, including a lone surrogate', () => {
    const text = 'a\u00e9\u20ac\u{1F600}\ud800z';
    expect(utf8ByteLength(text)).toBe(new TextEncoder().encode(text).length);
  });
});

describe('clipboardPreview', () => {
  it('is null without a match or for another provider', () => {
    expect(clipboardPreview(undefined)).toBeNull();
    expect(clipboardPreview({ ...textMatch('x'), provider: 'desktop-apps' })).toBeNull();
  });

  it('previews the full copy text with age, size and line count', () => {
    expect(clipboardPreview(textMatch('first line\nsecond line'))).toEqual({
      kind: 'text',
      text: 'first line\nsecond line',
      hiddenCharacters: 0,
      details: '5m ago · 22 bytes · 2 lines',
    });
  });

  it('counts UTF-8 bytes and says line for one line', () => {
    expect(clipboardPreview(textMatch('é'))?.details).toBe('5m ago · 2 bytes · 1 line');
  });

  it('counts a surrogate pair as four bytes', () => {
    expect(clipboardPreview(textMatch('\u{1F600}'))?.details).toBe('5m ago · 4 bytes · 1 line');
  });

  it('shows text up to the character limit in full', () => {
    const text = 'x'.repeat(PREVIEW_CHARACTER_LIMIT);
    const preview = clipboardPreview(textMatch(text));
    expect(preview?.kind).toBe('text');
    if (preview?.kind === 'text') {
      expect(preview.text.length).toBe(PREVIEW_CHARACTER_LIMIT);
      expect(preview.hiddenCharacters).toBe(0);
    }
  });

  it('truncates text past the character limit but describes the full copy', () => {
    const text = 'x'.repeat(PREVIEW_CHARACTER_LIMIT + 5);
    const preview = clipboardPreview(textMatch(text));
    expect(preview?.kind).toBe('text');
    if (preview?.kind === 'text') {
      expect(preview.text).toBe('x'.repeat(PREVIEW_CHARACTER_LIMIT));
      expect(preview.hiddenCharacters).toBe(5);
      expect(preview.details).toBe('5m ago · 97.7 KB · 1 line');
    }
  });

  it('previews an image entry from its thumbnail', () => {
    const match: Match = {
      id: '2',
      provider: 'clipboard',
      title: 'Image 1280x720',
      subtitle: '1h ago',
      icon: { kind: 'data_uri', data: 'data:image/png;base64,AAAA' },
      score: 1,
      action: { kind: 'custom', data: { kind: 'clipboard', payload: { op: 'recopy', id: '2' } } },
    };
    expect(clipboardPreview(match)).toEqual({
      kind: 'image',
      source: 'data:image/png;base64,AAAA',
      details: '1h ago · Image 1280x720',
    });
  });

  it('falls back to age and title for other entries', () => {
    const match: Match = {
      id: '3',
      provider: 'clipboard',
      title: 'blob.bin',
      subtitle: '2d ago',
      score: 1,
      action: { kind: 'custom', data: { kind: 'clipboard', payload: { op: 'recopy', id: '3' } } },
    };
    expect(clipboardPreview(match)).toEqual({ kind: 'other', details: '2d ago · blob.bin' });
  });
});
