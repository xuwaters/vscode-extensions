import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  DecodeIndexError,
  INDEX_HEADER_SIZE,
  cacheKey,
  decodeIndexFile,
  encodeIndexFile,
  evictCache,
  pathHashOf,
  resolveCachePath,
  tryReadIndexFile,
  writeIndexFile,
} from './cache.js';

function tmpdir(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'logviewer-cache-'));
}

describe('cache', () => {
  let dir: string;
  beforeEach(() => {
    dir = tmpdir();
  });
  afterEach(() => {
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it('round-trips a small index file', () => {
    const anchors = new BigUint64Array([0n, 1024n, 2048n, 1n << 40n]);
    const header = {
      stride: 1024,
      totalLines: 4_096,
      fileSize: 12_345,
      mtimeMs: 1_700_000_000_000,
      pathHash: pathHashOf('/var/log/x.log'),
      anchorCount: anchors.length,
    };
    const buf = encodeIndexFile({ header, anchors });
    expect(buf.length).toBe(INDEX_HEADER_SIZE + anchors.length * 8);
    const decoded = decodeIndexFile(buf);
    expect(decoded.header.stride).toBe(1024);
    expect(decoded.header.totalLines).toBe(4_096);
    expect(decoded.header.pathHash.equals(header.pathHash)).toBe(true);
    expect(Array.from(decoded.anchors)).toEqual(Array.from(anchors));
  });

  it('rejects bad magic / version / size drift', () => {
    const anchors = new BigUint64Array([0n]);
    const buf = encodeIndexFile({
      header: {
        stride: 1,
        totalLines: 1,
        fileSize: 100,
        mtimeMs: 0,
        pathHash: pathHashOf('p'),
        anchorCount: 1,
      },
      anchors,
    });
    // Bad magic
    const bad = Buffer.from(buf);
    bad[0] = 0;
    expect(() => decodeIndexFile(bad)).toThrowError(DecodeIndexError);
    // Size mismatch
    expect(() => decodeIndexFile(buf, { expectedSize: 999 })).toThrowError(
      /size drifted/,
    );
    // Path mismatch
    expect(() =>
      decodeIndexFile(buf, { expectedPathHash: pathHashOf('other') }),
    ).toThrowError(/path hash mismatch/);
  });

  it('cacheKey is stable for the same file, distinct for different size/mtime', () => {
    const a = cacheKey('/p/x.log', 100, 1000);
    const b = cacheKey('/p/x.log', 100, 1000);
    const c = cacheKey('/p/x.log', 101, 1000);
    const d = cacheKey('/p/x.log', 100, 1001);
    expect(a).toBe(b);
    expect(a).not.toBe(c);
    expect(a).not.toBe(d);
  });

  it('resolveCachePath handles all three modes', () => {
    const paths = {
      globalStorageDir: '/storage/index',
      customDir: '/custom',
    };
    const globalP = resolveCachePath('globalStorage', '/var/log/x.log', 1, 2, paths);
    expect(globalP.startsWith('/storage/index/index/')).toBe(true);
    expect(globalP.endsWith('.idx')).toBe(true);
    const adjP = resolveCachePath('adjacent', '/var/log/x.log', 1, 2, paths);
    expect(adjP).toBe('/var/log/.x.log.idx');
    const dirP = resolveCachePath('directory', '/var/log/x.log', 1, 2, paths);
    expect(dirP.startsWith('/custom/')).toBe(true);
  });

  it('writes and reads back via tryReadIndexFile', () => {
    const filePath = path.join(dir, 'sub', 'idx.idx');
    const anchors = new BigUint64Array([0n, 100n, 200n]);
    writeIndexFile(filePath, {
      header: {
        stride: 100,
        totalLines: 300,
        fileSize: 1000,
        mtimeMs: 12345,
        pathHash: pathHashOf('a'),
        anchorCount: anchors.length,
      },
      anchors,
    });
    const back = tryReadIndexFile(filePath);
    expect(back).not.toBeNull();
    expect(Array.from(back!.anchors)).toEqual([0n, 100n, 200n]);
    expect(back!.header.fileSize).toBe(1000);
  });

  it('tryReadIndexFile returns null on missing or invalid', () => {
    expect(tryReadIndexFile(path.join(dir, 'nope.idx'))).toBeNull();
    const bad = path.join(dir, 'bad.idx');
    fs.writeFileSync(bad, Buffer.alloc(10));
    expect(tryReadIndexFile(bad)).toBeNull();
  });

  it('evictCache drops oldest atime files past budget', () => {
    // Write 5 .idx files of 100 bytes each, with staggered atimes.
    const sizes: number[] = [];
    for (let i = 0; i < 5; i++) {
      const fp = path.join(dir, `f${i}.idx`);
      fs.writeFileSync(fp, Buffer.alloc(100));
      // Stagger atime so f0 is oldest, f4 newest.
      const atime = new Date(Date.now() - (5 - i) * 1000);
      const mtime = atime;
      fs.utimesSync(fp, atime, mtime);
      sizes.push(100);
    }
    const result = evictCache(dir, 250);
    expect(result.scannedBytes).toBe(500);
    expect(result.removedFiles).toBeGreaterThanOrEqual(2);
    // f0 should be gone; f4 should still exist.
    expect(fs.existsSync(path.join(dir, 'f4.idx'))).toBe(true);
    expect(fs.existsSync(path.join(dir, 'f0.idx'))).toBe(false);
  });

  it('evictCache no-op when under budget', () => {
    fs.writeFileSync(path.join(dir, 'a.idx'), Buffer.alloc(100));
    const r = evictCache(dir, 1000);
    expect(r.removedFiles).toBe(0);
    expect(r.removedBytes).toBe(0);
  });
});
