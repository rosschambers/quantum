import { describe, it, expect } from 'vitest';
import { providersForQuery } from './prefixMode';

describe('providersForQuery', () => {
  it('pins an empty query to the desktop-apps provider', () => {
    expect(providersForQuery('')).toEqual(['desktop-apps']);
  });

  it('pins a whitespace-only query to the desktop-apps provider', () => {
    expect(providersForQuery('   ')).toEqual(['desktop-apps']);
  });

  it('pins an = prefixed query to the calc provider', () => {
    expect(providersForQuery('=2+2')).toEqual(['calc']);
  });

  it('treats bare math starting with a digit as calc', () => {
    expect(providersForQuery('2+2')).toEqual(['calc']);
  });

  it('treats bare math starting with an opening parenthesis as calc', () => {
    expect(providersForQuery('(1+2)*3')).toEqual(['calc']);
  });

  it('treats bare math starting with a minus as calc', () => {
    expect(providersForQuery('-5*3')).toEqual(['calc']);
  });

  it('does not treat a bare number without an operator as calc', () => {
    expect(providersForQuery('42')).toEqual([]);
  });

  it('pins a : prefixed query to the emoji provider', () => {
    expect(providersForQuery(':smile')).toEqual(['emoji']);
  });

  it('pins a ; prefixed query to the clipboard provider', () => {
    expect(providersForQuery(';foo')).toEqual(['clipboard']);
  });

  it('pins a > command query to the shell provider', () => {
    expect(providersForQuery('>kill quantumd')).toEqual(['shell']);
  });

  it('pins a ! command query to the shell provider', () => {
    expect(providersForQuery('!htop')).toEqual(['shell']);
  });

  it('fans a plain query out to all providers', () => {
    expect(providersForQuery('firefox')).toEqual([]);
  });
});
