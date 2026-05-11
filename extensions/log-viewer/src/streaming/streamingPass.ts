// Streaming filter and search passes.
//
// Walk the file in chunks aligned to anchor boundaries, invoke the WASM
// `matchLines` / `searchLines` exports on each slab, and emit results in
// batches. Runs on the extension host main thread but yields between
// chunks via `setImmediate` so the host stays responsive and remains
// cancellable mid-scan.

import * as fs from 'fs';
import type { FilterRule, WasmModule } from '../types.js';

export interface ChunkBoundary {
  /** Byte start of the chunk (= byte offset of `lineStart`). */
  byteStart: number;
  byteEnd: number;
  lineStart: number;
  lineEnd: number;
}

export function planChunks(
  anchors: BigUint64Array,
  stride: number,
  totalLines: number,
  fileSize: number,
  maxChunkBytes: number,
): ChunkBoundary[] {
  const out: ChunkBoundary[] = [];
  if (anchors.length === 0) return out;
  let i = 0;
  while (i < anchors.length) {
    const byteStart = Number(anchors[i]);
    let j = i + 1;
    while (
      j < anchors.length &&
      Number(anchors[j] - anchors[i]) < maxChunkBytes
    ) {
      j += 1;
    }
    const lineStart = i * stride;
    const lineEnd = j < anchors.length ? j * stride : totalLines;
    const byteEnd = j < anchors.length ? Number(anchors[j]) : fileSize;
    out.push({ byteStart, byteEnd, lineStart, lineEnd });
    i = j;
  }
  return out;
}

function readSlab(fd: number, byteStart: number, byteEnd: number): Buffer {
  const len = Math.max(0, byteEnd - byteStart);
  const buf = Buffer.allocUnsafe(len);
  let total = 0;
  while (total < len) {
    const n = fs.readSync(fd, buf, total, len - total, byteStart + total);
    if (n <= 0) break;
    total += n;
  }
  return total === len ? buf : buf.subarray(0, total);
}

function yieldToEventLoop(): Promise<void> {
  return new Promise((resolve) => setImmediate(resolve));
}

export interface FilterHit {
  /** Global line number. */
  line: number;
  /** Rule index (0-based). */
  rule: number;
}

export interface FilterPassOptions {
  fd: number;
  wasm: WasmModule;
  anchors: BigUint64Array;
  stride: number;
  totalLines: number;
  fileSize: number;
  rules: FilterRule[];
  chunkBytes?: number;
  /** Hit cap; results past this are dropped, `truncated=true` is reported. */
  maxHits?: number;
  /** Emit batched hits as they arrive. */
  onProgress?: (e: {
    scannedBytes: number;
    scannedLines: number;
    hits: FilterHit[];
  }) => void;
  onDone?: (e: { totalHits: number; truncated: boolean }) => void;
  signal?: { aborted: boolean };
}

export async function runFilterPass(opts: FilterPassOptions): Promise<void> {
  const chunkBytes = opts.chunkBytes ?? 4 * 1024 * 1024;
  const maxHits = opts.maxHits ?? 1_000_000;
  const rulesJson = JSON.stringify(opts.rules);
  const chunks = planChunks(
    opts.anchors,
    opts.stride,
    opts.totalLines,
    opts.fileSize,
    chunkBytes,
  );
  let totalHits = 0;
  let truncated = false;
  let scannedBytes = 0;
  let scannedLines = 0;

  for (const ch of chunks) {
    if (opts.signal?.aborted) break;
    const slab = readSlab(opts.fd, ch.byteStart, ch.byteEnd);
    const tags = opts.wasm.matchLines(new Uint8Array(slab), rulesJson);
    const expected = ch.lineEnd - ch.lineStart;
    const cap = Math.min(tags.length, expected);
    const batch: FilterHit[] = [];
    for (let k = 0; k < cap; k++) {
      const tag = tags[k];
      if (tag > 0) {
        if (totalHits >= maxHits) {
          truncated = true;
          break;
        }
        batch.push({ line: ch.lineStart + k, rule: tag - 1 });
        totalHits += 1;
      }
    }
    scannedBytes = ch.byteEnd;
    scannedLines = ch.lineEnd;
    opts.onProgress?.({ scannedBytes, scannedLines, hits: batch });
    if (truncated) break;
    await yieldToEventLoop();
  }
  opts.onDone?.({ totalHits, truncated });
}

export interface SearchPassOptions {
  fd: number;
  wasm: WasmModule;
  anchors: BigUint64Array;
  stride: number;
  totalLines: number;
  fileSize: number;
  query: string;
  regex: boolean;
  caseSensitive: boolean;
  chunkBytes?: number;
  maxHits?: number;
  onProgress?: (e: {
    scannedBytes: number;
    scannedLines: number;
    hits: number[];
  }) => void;
  onDone?: (e: { totalHits: number; truncated: boolean }) => void;
  signal?: { aborted: boolean };
}

export async function runSearchPass(opts: SearchPassOptions): Promise<void> {
  const chunkBytes = opts.chunkBytes ?? 4 * 1024 * 1024;
  const maxHits = opts.maxHits ?? 1_000_000;
  const chunks = planChunks(
    opts.anchors,
    opts.stride,
    opts.totalLines,
    opts.fileSize,
    chunkBytes,
  );
  let totalHits = 0;
  let truncated = false;
  let scannedBytes = 0;
  let scannedLines = 0;

  for (const ch of chunks) {
    if (opts.signal?.aborted) break;
    const slab = readSlab(opts.fd, ch.byteStart, ch.byteEnd);
    const localHits = opts.wasm.searchLines(
      new Uint8Array(slab),
      opts.query,
      opts.regex,
      opts.caseSensitive,
    );
    const expected = ch.lineEnd - ch.lineStart;
    const batch: number[] = [];
    for (let k = 0; k < localHits.length; k++) {
      const local = localHits[k];
      if (local >= expected) continue;
      if (totalHits >= maxHits) {
        truncated = true;
        break;
      }
      batch.push(ch.lineStart + local);
      totalHits += 1;
    }
    scannedBytes = ch.byteEnd;
    scannedLines = ch.lineEnd;
    opts.onProgress?.({ scannedBytes, scannedLines, hits: batch });
    if (truncated) break;
    await yieldToEventLoop();
  }
  opts.onDone?.({ totalHits, truncated });
}
