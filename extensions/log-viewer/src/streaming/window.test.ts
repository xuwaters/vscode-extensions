import { describe, expect, it } from 'vitest';
import { WindowCache, planWindow } from './window.js';

describe('planWindow', () => {
  const idx = {
    // 10 lines of "x\n" each: byte offsets 0, 2, 4, 6, 8, 10, 12, 14, 16, 18.
    // stride=2 → anchors are at lines 0,2,4,6,8,10 → bytes 0,4,8,12,16,20.
    anchors: new BigUint64Array([0n, 4n, 8n, 12n, 16n, 20n]),
    stride: 2,
    totalLines: 11, // 10 newlines → 11 lines (last one empty)
    fileSize: 20,
  };

  it('plans a small window inside one stride', () => {
    const p = planWindow(idx, 0, 2)!;
    expect(p.byteStart).toBe(0);
    expect(p.byteEnd).toBe(4); // anchors[1]
    expect(p.firstLineInSlab).toBe(0);
    expect(p.linesInSlab).toBe(2);
    expect(p.localStart).toBe(0);
    expect(p.localEnd).toBe(2);
  });

  it('plans a window crossing an anchor', () => {
    const p = planWindow(idx, 1, 5)!;
    // startAnchorIdx=0, endAnchorIdx=ceil(5/2)=3.
    expect(p.byteStart).toBe(0);
    expect(p.byteEnd).toBe(12); // anchors[3]
    expect(p.firstLineInSlab).toBe(0);
    expect(p.linesInSlab).toBe(6);
    expect(p.localStart).toBe(1);
    expect(p.localEnd).toBe(5);
  });

  it('clamps end at totalLines and reads to fileSize', () => {
    const p = planWindow(idx, 8, 999)!;
    expect(p.byteStart).toBe(16); // anchors[4]
    expect(p.byteEnd).toBe(20); // fileSize (past last anchor)
    expect(p.firstLineInSlab).toBe(8);
    expect(p.localStart).toBe(0);
    // clamped end = 11 → local 3 (8,9,10).
    expect(p.localEnd).toBe(3);
  });

  it('returns null for empty range', () => {
    expect(planWindow(idx, 5, 5)).toBeNull();
  });

  it('returns null for out-of-range start', () => {
    expect(planWindow(idx, 11, 12)).toBeNull();
    expect(planWindow(idx, -1, 1)).toBeNull();
  });
});

describe('WindowCache', () => {
  it('stores up to capacity then evicts oldest', () => {
    const c = new WindowCache<number>(2);
    c.set('a', 1);
    c.set('b', 2);
    c.set('c', 3);
    expect(c.get('a')).toBeUndefined();
    expect(c.get('b')).toBe(2);
    expect(c.get('c')).toBe(3);
  });

  it('refreshes recency on get', () => {
    const c = new WindowCache<number>(2);
    c.set('a', 1);
    c.set('b', 2);
    c.get('a'); // a is now most recent
    c.set('c', 3); // b should be evicted
    expect(c.get('b')).toBeUndefined();
    expect(c.get('a')).toBe(1);
    expect(c.get('c')).toBe(3);
  });
});
