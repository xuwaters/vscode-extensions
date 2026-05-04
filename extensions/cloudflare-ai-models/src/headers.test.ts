import { describe, expect, it } from 'vitest';
import { effectiveModelUrl, mergeHeaders } from './headers.js';

describe('effectiveModelUrl', () => {
  it('uses model url when set', () => {
    expect(effectiveModelUrl({url: 'https://m' }, 'https://p')).toBe('https://m');
  });
  it('falls back to provider url when model url is empty', () => {
    expect(effectiveModelUrl({}, 'https://p')).toBe('https://p');
    expect(effectiveModelUrl({url: '   ' }, 'https://p')).toBe('https://p');
  });
});

describe('mergeHeaders', () => {
  it('merges later layers on top of earlier ones', () => {
    expect(mergeHeaders({ a: '1', b: '2' }, { b: '3', c: '4' })).toEqual({ a: '1', b: '3', c: '4' });
  });
  it('drops reserved headers regardless of casing', () => {
    expect(
      mergeHeaders({ Authorization: 'leak', 'Content-Type': 'x', 'X-Trace': 't' }),
    ).toEqual({ 'X-Trace': 't' });
  });
  it('ignores undefined layers', () => {
    expect(mergeHeaders(undefined, { a: '1' }, undefined)).toEqual({ a: '1' });
  });
});
