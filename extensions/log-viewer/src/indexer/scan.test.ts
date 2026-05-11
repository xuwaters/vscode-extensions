import { describe, expect, it } from 'vitest';
import { scanIndex } from './scan.js';

function fakeRead(source: Buffer) {
  return (offset: bigint, buf: Buffer, len: number): number => {
    const start = Number(offset);
    if (start >= source.length) return 0;
    const end = Math.min(source.length, start + len);
    const n = end - start;
    source.copy(buf, 0, start, end);
    return n;
  };
}

function collectAnchors(emitted: BigUint64Array[]): bigint[] {
  const all: bigint[] = [];
  for (const arr of emitted) {
    for (let i = 0; i < arr.length; i++) all.push(arr[i]);
  }
  return all;
}

describe('scanIndex', () => {
  it('emits anchor 0 for the head of the file', () => {
    const src = Buffer.from('first\nsecond\nthird\n');
    const emitted: BigUint64Array[] = [];
    const res = scanIndex({
      stride: 4,
      chunkSize: 1024,
      read: fakeRead(src),
      emitAnchors: (a) => emitted.push(a),
    });
    expect(res.cancelled).toBe(false);
    expect(res.fileSize).toBe(BigInt(src.length));
    // 3 newlines → 4 lines (the trailing empty line after the last \n).
    expect(res.totalLines).toBe(4);
    const anchors = collectAnchors(emitted);
    expect(anchors).toEqual([0n]);
  });

  it('records an anchor every Nth line', () => {
    // 10 lines: byte offsets of each line start are 0, 2, 4, 6, 8, 10, 12, 14, 16, 18.
    const src = Buffer.from('a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n');
    const emitted: BigUint64Array[] = [];
    const res = scanIndex({
      stride: 3,
      chunkSize: 1024,
      read: fakeRead(src),
      emitAnchors: (a) => emitted.push(a),
    });
    // With stride 3 we record anchors at line 0, 3, 6, 9, 12, ...
    // Line 3 starts at byte 6; line 6 at byte 12; line 9 at byte 18; line 12 doesn't exist.
    const anchors = collectAnchors(emitted);
    expect(anchors).toEqual([0n, 6n, 12n, 18n]);
    expect(res.totalLines).toBe(11); // 10 newlines + 1
  });

  it('handles a file with no trailing newline', () => {
    const src = Buffer.from('a\nb\nc'); // 3 lines, 2 newlines
    const emitted: BigUint64Array[] = [];
    const res = scanIndex({
      stride: 1,
      chunkSize: 1024,
      read: fakeRead(src),
      emitAnchors: (a) => emitted.push(a),
    });
    expect(res.totalLines).toBe(3);
    expect(res.fileSize).toBe(5n);
    expect(collectAnchors(emitted)).toEqual([0n, 2n, 4n]);
  });

  it('produces identical anchors regardless of chunk boundary', () => {
    // 100 short lines.
    const lines: string[] = [];
    for (let i = 0; i < 100; i++) lines.push(`line-${i}`);
    const src = Buffer.from(lines.join('\n') + '\n');
    const results: bigint[][] = [];
    for (const chunkSize of [1, 3, 7, 32, 256, 4096]) {
      const emitted: BigUint64Array[] = [];
      scanIndex({
        stride: 10,
        chunkSize,
        read: fakeRead(src),
        emitAnchors: (a) => emitted.push(a),
      });
      results.push(collectAnchors(emitted));
    }
    // All chunk sizes should produce the same anchor list.
    for (let i = 1; i < results.length; i++) {
      expect(results[i]).toEqual(results[0]);
    }
  });

  it('flushes anchors in batches', () => {
    const lines: string[] = [];
    for (let i = 0; i < 20; i++) lines.push('x');
    const src = Buffer.from(lines.join('\n') + '\n');
    const emitted: BigUint64Array[] = [];
    scanIndex({
      // Small chunk forces multiple reads, which is when batch flushing
      // gets a chance to fire (the algorithm only flushes between chunks).
      stride: 1,
      chunkSize: 4,
      batchAnchors: 4,
      read: fakeRead(src),
      emitAnchors: (a) => emitted.push(a),
    });
    // 21 anchors total (lines 0..20), batched in groups of >= 4 plus a final.
    expect(emitted.length).toBeGreaterThan(1);
    expect(collectAnchors(emitted).length).toBe(21);
  });

  it('supports cancellation between chunks', () => {
    const src = Buffer.alloc(4096, 0x61); // all 'a', no newlines
    let calls = 0;
    const res = scanIndex({
      stride: 1,
      chunkSize: 16,
      read: fakeRead(src),
      isCancelled: () => ++calls >= 3,
      emitAnchors: () => {},
    });
    expect(res.cancelled).toBe(true);
  });

  it('handles empty file', () => {
    const src = Buffer.alloc(0);
    const emitted: BigUint64Array[] = [];
    const res = scanIndex({
      stride: 1,
      chunkSize: 1024,
      read: fakeRead(src),
      emitAnchors: (a) => emitted.push(a),
    });
    expect(res.totalLines).toBe(1);
    expect(res.fileSize).toBe(0n);
    expect(collectAnchors(emitted)).toEqual([0n]);
  });

  it('accumulates byte offsets with BigInt math', () => {
    // Synthesize > 4 GB of bytes by lying about chunk sizes. Each chunk
    // returns a buffer with a single newline at the start, but reports a
    // huge `n` so that fileOffset must use BigInt arithmetic to advance
    // correctly past 2^32.
    const hugeChunk = 1n << 31n; // 2 GB
    const totalChunks = 3;
    let chunksReturned = 0;
    const reads: bigint[] = [];
    const emitted: bigint[] = [];
    scanIndex({
      stride: 1,
      chunkSize: 4,
      read: (offset, buf) => {
        reads.push(offset);
        if (chunksReturned >= totalChunks) return 0;
        buf[0] = 0x0a; // '\n' at local byte 0
        chunksReturned += 1;
        return Number(hugeChunk);
      },
      emitAnchors: (a) => {
        for (let i = 0; i < a.length; i++) emitted.push(a[i]);
      },
    });
    // 3 newlines => totalLines = 4. After 3 chunks fileSize = 6 GB.
    expect(reads[0]).toBe(0n);
    expect(reads[1]).toBe(hugeChunk);
    expect(reads[2]).toBe(2n * hugeChunk);
    // anchors record byte offsets of line starts:
    //   line 0 → 0
    //   line 1 → 1 (just after the first \n at offset 0)
    //   line 2 → 2 GB + 1
    //   line 3 → 4 GB + 1
    expect(emitted).toEqual([0n, 1n, hugeChunk + 1n, 2n * hugeChunk + 1n]);
  });
});
