import { describe, it, expect } from 'vitest';
import { parseCommandQuery } from './commandQuery';

describe('parseCommandQuery', () => {
  it('parses a $ query into capture mode with the command trimmed', () => {
    expect(parseCommandQuery('$echo hi')).toEqual({ mode: 'capture', command: 'echo hi' });
  });

  it('trims surrounding whitespace after the $ prefix', () => {
    expect(parseCommandQuery('$   echo hi   ')).toEqual({ mode: 'capture', command: 'echo hi' });
  });

  it('returns none for a bare $ with only trailing whitespace', () => {
    expect(parseCommandQuery('$')).toEqual({ mode: 'none' });
    expect(parseCommandQuery('$   ')).toEqual({ mode: 'none' });
  });

  it('returns none for the > launch prefix', () => {
    expect(parseCommandQuery('>firefox')).toEqual({ mode: 'none' });
  });

  it('returns none for the ! terminal prefix', () => {
    expect(parseCommandQuery('!htop')).toEqual({ mode: 'none' });
  });

  it('returns none for a plain query', () => {
    expect(parseCommandQuery('firefox')).toEqual({ mode: 'none' });
  });

  it('returns none for an empty string', () => {
    expect(parseCommandQuery('')).toEqual({ mode: 'none' });
  });
});
