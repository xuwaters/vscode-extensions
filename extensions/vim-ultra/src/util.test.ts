import { describe, expect, it } from 'vitest';
import { modeLabel, replaceEol, serializeSelections, typedKeys } from './util';

describe('typedKeys', () => {
  it('splits per code point', () => {
    expect(typedKeys('ab')).toEqual(['a', 'b']);
    expect(typedKeys('a😀b')).toEqual(['a', '😀', 'b']);
    expect(typedKeys('')).toEqual([]);
  });
});

describe('replaceEol', () => {
  it('is identity for LF documents', () => {
    expect(replaceEol('a\nb', '\n')).toBe('a\nb');
  });
  it('rewrites to CRLF', () => {
    expect(replaceEol('a\nb\n', '\r\n')).toBe('a\r\nb\r\n');
  });
});

describe('serializeSelections', () => {
  it('round-trips equality checks', () => {
    const a = [{ anchor: { line: 0, character: 1 }, active: { line: 2, character: 3 } }];
    const b = [{ anchor: { line: 0, character: 1 }, active: { line: 2, character: 3 } }];
    expect(serializeSelections(a)).toBe(serializeSelections(b));
    b[0].active.character = 4;
    expect(serializeSelections(a)).not.toBe(serializeSelections(b));
  });
});

describe('modeLabel', () => {
  it('labels modes and appends pending keys', () => {
    expect(modeLabel('normal', '')).toBe('-- NORMAL --');
    expect(modeLabel('insert', '')).toBe('-- INSERT --');
    expect(modeLabel('visualLine', '')).toBe('-- VISUAL LINE --');
    expect(modeLabel('normal', '2d')).toBe('-- NORMAL -- 2d');
  });
  it('appends engine messages', () => {
    expect(modeLabel('normal', '', '2 substitutions on 1 line')).toBe(
      '-- NORMAL -- 2 substitutions on 1 line',
    );
    expect(modeLabel('normal', ':s/a/b/', '')).toBe('-- NORMAL -- :s/a/b/');
  });
});
