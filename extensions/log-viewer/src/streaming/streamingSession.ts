// Per-document streaming session: owns the indexer, the file descriptor,
// the in-memory anchor array, and the rendered-window cache. Drives the
// webview via the host's `postMessage` callback.

import * as fs from 'fs';
import {
  DEFAULT_BATCH_ANCHORS,
  DEFAULT_CHUNK_SIZE,
  DEFAULT_STRIDE,
  Indexer,
} from '../indexer/indexer.js';
import {
  type IndexLocationMode,
  evictCache,
  globalIndexDir,
  pathHashOf,
  resolveCachePath,
  tryReadIndexFile,
  writeIndexFile,
} from '../indexer/cache.js';
import { type IndexView, WindowCache, planWindow } from './window.js';
import {
  type FilterHit,
  runFilterPass,
  runSearchPass,
} from './streamingPass.js';
import type { FilterRule, ParsedLines, WasmModule } from '../types.js';

export interface LineRecord {
  html: string;
  text: string;
}

export interface IndexProgressEvent {
  /** Newlines seen so far. Final value = totalLines - 1. */
  scannedLines: number;
  /** Bytes scanned so far. */
  scannedBytes: number;
  complete: boolean;
  /** Total line count if known (set on complete; may be undefined earlier). */
  totalLines?: number;
  /** Total bytes (from stat at open time). */
  fileSize: number;
}

export interface StreamingSessionOptions {
  fsPath: string;
  wasm: WasmModule;
  cache: {
    globalStorageDir: string;
    customDir?: string;
    mode: IndexLocationMode;
    budgetBytes: number;
  };
  stride?: number;
  chunkSize?: number;
  batchAnchors?: number;
  /** Worker bundle path override (for tests). */
  workerScript?: string;
  /** Pluggable Indexer factory for tests. */
  indexerFactory?: () => Indexer;
  /** Invoked on every progress event, plus once on completion. */
  onIndexProgress: (event: IndexProgressEvent) => void;
  /** Reports unrecoverable errors (the session is then in an error state). */
  onError: (err: Error) => void;
}

export interface WindowResult {
  /** First line in the result. */
  start: number;
  /** Rendered records, in order. */
  lines: LineRecord[];
  /** True if the requested range exceeds what the index currently covers; `lines` may be shorter. */
  partial: boolean;
}

export class StreamingSession {
  private readonly opts: StreamingSessionOptions;
  private readonly indexer: Indexer;
  private readonly anchors: bigint[] = [];
  private readonly windowCache = new WindowCache<LineRecord[]>(16);
  private readonly stride: number;
  private fd: number | null = null;
  private fileSize = 0;
  private mtimeMs = 0;
  private totalLines = 1; // updated on complete; pre-complete we expose anchors-based estimate
  private indexComplete = false;
  private disposed = false;
  private adjacentFallback = false;

  constructor(opts: StreamingSessionOptions) {
    this.opts = opts;
    this.indexer = opts.indexerFactory ? opts.indexerFactory() : new Indexer();
    this.stride = opts.stride ?? DEFAULT_STRIDE;
  }

  async start(): Promise<void> {
    const st = fs.statSync(this.opts.fsPath);
    this.fileSize = st.size;
    this.mtimeMs = Math.floor(st.mtimeMs);
    this.fd = fs.openSync(this.opts.fsPath, 'r');

    // Try cache first.
    const cached = this.tryLoadCache();
    if (cached) {
      this.anchors.length = 0;
      for (let i = 0; i < cached.anchors.length; i++) {
        this.anchors.push(cached.anchors[i]);
      }
      this.totalLines = cached.header.totalLines;
      this.indexComplete = true;
      this.opts.onIndexProgress({
        scannedLines: cached.header.totalLines - 1,
        scannedBytes: cached.header.fileSize,
        complete: true,
        totalLines: cached.header.totalLines,
        fileSize: this.fileSize,
      });
      return;
    }

    this.indexer.start({
      path: this.opts.fsPath,
      stride: this.stride,
      chunkSize: this.opts.chunkSize ?? DEFAULT_CHUNK_SIZE,
      batchAnchors: this.opts.batchAnchors ?? DEFAULT_BATCH_ANCHORS,
      workerScript: this.opts.workerScript,
      onProgress: ({ lines, bytes, newAnchors }) => {
        if (this.disposed) return;
        for (let i = 0; i < newAnchors.length; i++) this.anchors.push(newAnchors[i]);
        // Lower bound on totalLines while we scan.
        if (this.anchors.length > 0) {
          this.totalLines = Math.max(this.totalLines, lines + 1);
        }
        this.opts.onIndexProgress({
          scannedLines: lines,
          scannedBytes: bytes,
          complete: false,
          fileSize: this.fileSize,
        });
      },
      onComplete: ({ totalLines, fileSize }) => {
        if (this.disposed) return;
        this.totalLines = totalLines;
        this.fileSize = fileSize;
        this.indexComplete = true;
        this.persistCache();
        this.opts.onIndexProgress({
          scannedLines: totalLines - 1,
          scannedBytes: fileSize,
          complete: true,
          totalLines,
          fileSize,
        });
      },
      onError: (err) => {
        if (this.disposed) return;
        this.opts.onError(err);
      },
    });
  }

  private tryLoadCache() {
    try {
      const cachePath = resolveCachePath(
        this.opts.cache.mode,
        this.opts.fsPath,
        this.fileSize,
        this.mtimeMs,
        {
          globalStorageDir: this.opts.cache.globalStorageDir,
          customDir: this.opts.cache.customDir,
        },
      );
      return tryReadIndexFile(cachePath, {
        expectedSize: this.fileSize,
        expectedMtimeMs: this.mtimeMs,
        expectedPathHash: pathHashOf(this.opts.fsPath),
      });
    } catch {
      return null;
    }
  }

  private persistCache(): void {
    const fileSize = this.fileSize;
    const anchorsArr = new BigUint64Array(this.anchors.length);
    for (let i = 0; i < this.anchors.length; i++) anchorsArr[i] = this.anchors[i];
    let targetPath: string;
    try {
      targetPath = resolveCachePath(
        this.opts.cache.mode,
        this.opts.fsPath,
        fileSize,
        this.mtimeMs,
        {
          globalStorageDir: this.opts.cache.globalStorageDir,
          customDir: this.opts.cache.customDir,
        },
      );
    } catch {
      // Misconfiguration — fall back silently.
      targetPath = resolveCachePath(
        'globalStorage',
        this.opts.fsPath,
        fileSize,
        this.mtimeMs,
        {
          globalStorageDir: this.opts.cache.globalStorageDir,
        },
      );
    }
    const write = (filePath: string): void => {
      writeIndexFile(filePath, {
        header: {
          stride: this.stride,
          totalLines: this.totalLines,
          fileSize: this.fileSize,
          mtimeMs: this.mtimeMs,
          pathHash: pathHashOf(this.opts.fsPath),
          anchorCount: anchorsArr.length,
        },
        anchors: anchorsArr,
      });
    };
    try {
      write(targetPath);
    } catch (e) {
      const code = (e as NodeJS.ErrnoException).code;
      if (
        this.opts.cache.mode === 'adjacent' &&
        (code === 'EACCES' || code === 'EROFS' || code === 'EPERM')
      ) {
        // Fall back per RFC §5.4.2.
        this.adjacentFallback = true;
        const fb = resolveCachePath(
          'globalStorage',
          this.opts.fsPath,
          fileSize,
          this.mtimeMs,
          { globalStorageDir: this.opts.cache.globalStorageDir },
        );
        try {
          write(fb);
        } catch {
          // give up silently
        }
      }
      // Other errors: don't fail the session over a cache write.
    }
    // Best-effort eviction in the centralised directories.
    if (this.opts.cache.mode !== 'adjacent') {
      const dir =
        this.opts.cache.mode === 'directory' && this.opts.cache.customDir
          ? this.opts.cache.customDir
          : globalIndexDir(this.opts.cache.globalStorageDir);
      try {
        evictCache(dir, this.opts.cache.budgetBytes);
      } catch {
        // ignore
      }
    }
  }

  /** Did the persistent cache write have to fall back to globalStorage? */
  hadAdjacentFallback(): boolean {
    return this.adjacentFallback;
  }

  view(): IndexView {
    const anchors = new BigUint64Array(this.anchors.length);
    for (let i = 0; i < this.anchors.length; i++) anchors[i] = this.anchors[i];
    return {
      anchors,
      stride: this.stride,
      totalLines: this.totalLines,
      fileSize: this.fileSize,
    };
  }

  getFileSize(): number {
    return this.fileSize;
  }
  getTotalLines(): number {
    return this.totalLines;
  }
  getStride(): number {
    return this.stride;
  }
  isComplete(): boolean {
    return this.indexComplete;
  }

  /**
   * Read the bytes covering lines `[start, end)`. Returns a Buffer of
   * exactly `byteEnd - byteStart` bytes (or fewer at EOF).
   */
  private readSlab(byteStart: number, byteEnd: number): Buffer {
    if (this.fd === null) throw new Error('session not started');
    const len = Math.max(0, byteEnd - byteStart);
    const buf = Buffer.allocUnsafe(len);
    let total = 0;
    while (total < len) {
      const n = fs.readSync(this.fd, buf, total, len - total, byteStart + total);
      if (n <= 0) break;
      total += n;
    }
    return total === len ? buf : buf.subarray(0, total);
  }

  /** Render lines `[start, end)` from disk via WASM. */
  requestWindow(start: number, end: number): WindowResult {
    if (this.disposed) return { start, lines: [], partial: true };
    const view = this.view();
    const plan = planWindow(view, start, end);
    if (!plan) return { start, lines: [], partial: false };

    const cacheKey = `${start}:${end}`;
    const cached = this.windowCache.get(cacheKey);
    if (cached) return { start, lines: cached, partial: false };

    const slab = this.readSlab(plan.byteStart, plan.byteEnd);
    const json = this.opts.wasm.renderLines(new Uint8Array(slab));
    const parsed = JSON.parse(json) as ParsedLines;
    const all: LineRecord[] = parsed.text.map((t, i) => ({
      text: t,
      html: parsed.html[i] ?? '',
    }));
    const want = Math.min(plan.linesInSlab, all.length);
    const slice = all.slice(plan.localStart, Math.min(plan.localEnd, want));
    this.windowCache.set(cacheKey, slice);
    return {
      start,
      lines: slice,
      partial: slice.length < end - start,
    };
  }

  /** Invalidate window cache (e.g. on rules change for streaming filter). */
  invalidateWindows(): void {
    this.windowCache.clear();
  }

  /**
   * Start a streaming filter pass. The previous pass (if any) is signalled
   * to abort. Returns a `cancel` function the caller can use to interrupt
   * mid-flight.
   */
  startFilterPass(opts: {
    rules: FilterRule[];
    chunkBytes?: number;
    maxHits?: number;
    onProgress?: (e: {
      scannedBytes: number;
      scannedLines: number;
      hits: FilterHit[];
    }) => void;
    onDone?: (e: { totalHits: number; truncated: boolean }) => void;
  }): () => void {
    if (this.fd === null) throw new Error('session not started');
    const signal = { aborted: false };
    void runFilterPass({
      fd: this.fd,
      wasm: this.opts.wasm,
      anchors: this.view().anchors,
      stride: this.stride,
      totalLines: this.totalLines,
      fileSize: this.fileSize,
      rules: opts.rules,
      chunkBytes: opts.chunkBytes,
      maxHits: opts.maxHits,
      onProgress: opts.onProgress,
      onDone: opts.onDone,
      signal,
    });
    return () => {
      signal.aborted = true;
    };
  }

  /** Start a streaming search pass; same shape as `startFilterPass`. */
  startSearchPass(opts: {
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
  }): () => void {
    if (this.fd === null) throw new Error('session not started');
    const signal = { aborted: false };
    void runSearchPass({
      fd: this.fd,
      wasm: this.opts.wasm,
      anchors: this.view().anchors,
      stride: this.stride,
      totalLines: this.totalLines,
      fileSize: this.fileSize,
      query: opts.query,
      regex: opts.regex,
      caseSensitive: opts.caseSensitive,
      chunkBytes: opts.chunkBytes,
      maxHits: opts.maxHits,
      onProgress: opts.onProgress,
      onDone: opts.onDone,
      signal,
    });
    return () => {
      signal.aborted = true;
    };
  }

  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    try {
      this.indexer.cancel();
    } catch {
      // ignore
    }
    await this.indexer.dispose();
    if (this.fd !== null) {
      try {
        fs.closeSync(this.fd);
      } catch {
        // ignore
      }
      this.fd = null;
    }
    this.windowCache.clear();
  }
}
