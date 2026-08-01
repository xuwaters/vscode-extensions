import { describe, expect, it } from 'vitest';
import {
  lineForOffset,
  normalize,
  offsetForLine,
  sourceposLine,
  type MapEntry,
} from './scrollMap';

const map: MapEntry[] = [
  { line: 0, top: 0 },
  { line: 10, top: 500 },
  { line: 20, top: 700 },
];

describe('sourceposLine', () => {
  it('parses the 1-based start line to 0-based', () => {
    expect(sourceposLine('12:1-14:8')).toBe(11);
    expect(sourceposLine('1:1-1:5')).toBe(0);
  });

  it('rejects garbage', () => {
    expect(sourceposLine(null)).toBeNull();
    expect(sourceposLine('x')).toBeNull();
    expect(sourceposLine('0:1-1:1')).toBeNull();
  });
});

describe('normalize', () => {
  it('sorts, drops duplicate lines and out-of-order offsets', () => {
    const entries: MapEntry[] = [
      { line: 5, top: 100 },
      { line: 1, top: 10 },
      { line: 5, top: 120 },
      { line: 7, top: 50 }, // out of order → dropped
      { line: 9, top: 200 },
    ];
    expect(normalize(entries)).toEqual([
      { line: 1, top: 10 },
      { line: 5, top: 100 },
      { line: 9, top: 200 },
    ]);
  });
});

describe('offsetForLine', () => {
  it('returns exact anchors', () => {
    expect(offsetForLine(map, 0)).toBe(0);
    expect(offsetForLine(map, 10)).toBe(500);
    expect(offsetForLine(map, 20)).toBe(700);
  });

  it('interpolates between anchors', () => {
    expect(offsetForLine(map, 5)).toBe(250);
    expect(offsetForLine(map, 15)).toBe(600);
  });

  it('clamps outside the map', () => {
    expect(offsetForLine(map, 100)).toBe(700);
    expect(offsetForLine([], 5)).toBeNull();
  });
});

describe('lineForOffset', () => {
  it('is the inverse of offsetForLine at anchors', () => {
    expect(lineForOffset(map, 0)).toBe(0);
    expect(lineForOffset(map, 500)).toBe(10);
    expect(lineForOffset(map, 700)).toBe(20);
  });

  it('interpolates between anchors', () => {
    expect(lineForOffset(map, 250)).toBe(5);
    expect(lineForOffset(map, 600)).toBe(15);
  });

  it('clamps outside the map', () => {
    expect(lineForOffset(map, -50)).toBe(0);
    expect(lineForOffset(map, 9999)).toBe(20);
    expect(lineForOffset([], 10)).toBeNull();
  });
});
