import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { loadWasm } from '../wasm.js';
import { planChunks, runFilterPass, runSearchPass } from './streamingPass.js';

const extensionPath = path.resolve(__dirname, '..', '..');
const wasm = loadWasm(extensionPath);

describe('planChunks', () => {
  it('packs anchors into byte-budgeted chunks', () => {
    // 10 anchors spaced 1000 bytes apart. stride=10, totalLines=100, fileSize=10000.
    const anchors = new BigUint64Array(10);
    for (let i = 0; i < 10; i++) anchors[i] = BigInt(i * 1000);
    const chunks = planChunks(anchors, 10, 100, 10_000, 3_000);
    // Each chunk should span at most 3 anchors (= 3000 bytes).
    expect(chunks.length).toBeGreaterThan(1);
    for (const c of chunks) {
      expect(c.byteEnd - c.byteStart).toBeLessThanOrEqual(3_000);
    }
    // Coverage must be complete and contiguous.
    expect(chunks[0].byteStart).toBe(0);
    expect(chunks[chunks.length - 1].byteEnd).toBe(10_000);
    for (let i = 1; i < chunks.length; i++) {
      expect(chunks[i].byteStart).toBe(chunks[i - 1].byteEnd);
      expect(chunks[i].lineStart).toBe(chunks[i - 1].lineEnd);
    }
    // Last chunk reaches totalLines.
    expect(chunks[chunks.length - 1].lineEnd).toBe(100);
  });

  it('returns an empty list for an empty anchor set', () => {
    expect(planChunks(new BigUint64Array(0), 1, 0, 0, 1000)).toEqual([]);
  });
});

describe.skipIf(!wasm)('runFilterPass / runSearchPass', () => {
  let tmpDir: string;
  let logPath: string;
  let fd = -1;
  const lineCount = 200;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'logviewer-pass-'));
    logPath = path.join(tmpDir, 'fixture.log');
    const lines: string[] = [];
    for (let i = 0; i < lineCount; i++) {
      const tag = i % 5 === 0 ? 'ERROR' : i % 5 === 1 ? 'WARN' : 'INFO';
      lines.push(`${tag} line ${i}`);
    }
    fs.writeFileSync(logPath, lines.join('\n') + '\n');
    fd = fs.openSync(logPath, 'r');
  });

  afterEach(() => {
    if (fd >= 0) fs.closeSync(fd);
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  function buildAnchors(stride: number): { anchors: BigUint64Array; totalLines: number; fileSize: number } {
    const buf = fs.readFileSync(logPath);
    const offsets: bigint[] = [0n];
    let lineNum = 0;
    for (let i = 0; i < buf.length; i++) {
      if (buf[i] === 0x0a) {
        lineNum += 1;
        if (lineNum % stride === 0) offsets.push(BigInt(i + 1));
      }
    }
    const arr = new BigUint64Array(offsets.length);
    for (let i = 0; i < offsets.length; i++) arr[i] = offsets[i];
    return { anchors: arr, totalLines: lineNum + 1, fileSize: buf.length };
  }

  it('runFilterPass returns the expected ERROR hits', async () => {
    const { anchors, totalLines, fileSize } = buildAnchors(16);
    const allHits: { line: number; rule: number }[] = [];
    let totalHits = 0;
    await runFilterPass({
      fd,
      wasm: wasm!,
      anchors,
      stride: 16,
      totalLines,
      fileSize,
      rules: [
        { name: 'errors', pattern: 'ERROR', regex: false, caseSensitive: true, enabled: true },
        { name: 'warns', pattern: 'WARN', regex: false, caseSensitive: true, enabled: true },
      ],
      chunkBytes: 256,
      onProgress: (e) => allHits.push(...e.hits),
      onDone: (e) => {
        totalHits = e.totalHits;
      },
    });
    const errorLines = allHits.filter((h) => h.rule === 0).map((h) => h.line);
    const warnLines = allHits.filter((h) => h.rule === 1).map((h) => h.line);
    // Every 5th line is ERROR; 1, 6, 11, … are WARN.
    expect(errorLines).toEqual(Array.from({ length: 40 }, (_, i) => i * 5));
    expect(warnLines).toEqual(Array.from({ length: 40 }, (_, i) => i * 5 + 1));
    expect(totalHits).toBe(80);
  });

  it('runSearchPass finds matching lines', async () => {
    const { anchors, totalLines, fileSize } = buildAnchors(16);
    const allHits: number[] = [];
    await runSearchPass({
      fd,
      wasm: wasm!,
      anchors,
      stride: 16,
      totalLines,
      fileSize,
      query: 'line 12',
      regex: false,
      caseSensitive: false,
      chunkBytes: 256,
      onProgress: (e) => allHits.push(...e.hits),
    });
    // Lines 12, 120..129 contain 'line 12' as a substring.
    expect(allHits).toContain(12);
    expect(allHits).toContain(120);
    expect(allHits).toContain(129);
    expect(allHits.length).toBe(11);
  });

  it('runFilterPass aborts on signal', async () => {
    const { anchors, totalLines, fileSize } = buildAnchors(4);
    const signal = { aborted: false };
    let progressCalls = 0;
    await runFilterPass({
      fd,
      wasm: wasm!,
      anchors,
      stride: 4,
      totalLines,
      fileSize,
      rules: [
        { name: 'errors', pattern: 'ERROR', regex: false, caseSensitive: true, enabled: true },
      ],
      chunkBytes: 32, // many tiny chunks
      onProgress: () => {
        progressCalls += 1;
        signal.aborted = true;
      },
      signal,
    });
    // We aborted after the first chunk, so progress shouldn't be called for every chunk.
    const chunks = planChunks(anchors, 4, totalLines, fileSize, 32);
    expect(progressCalls).toBeLessThan(chunks.length);
  });
});
