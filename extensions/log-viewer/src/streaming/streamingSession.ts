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
  /** Bytes to pre-read from the tail of the file for the "End" affordance. */
  tailBytes?: number;
  /** Bytes to pre-read from the head of the file for the first-page fast path. */
  headBytes?: number;
  /** Invoked on every progress event, plus once on completion. */
  onIndexProgress: (event: IndexProgressEvent) => void;
  /** Reports unrecoverable errors (the session is then in an error state). */
  onError: (err: Error) => void;
  /**
   * Fired when fs.watch reports the underlying file has changed in a way
   * that invalidates the index (size or mtime drift). The host typically
   * surfaces this as a reload prompt — RFC §6.1.
   */
  onFileChanged?: (event: {
    previousSize: number;
    currentSize: number;
    previousMtimeMs: number;
    currentMtimeMs: number;
  }) => void;
}

export interface WindowResult {
  /** First line in the result. */
  start: number;
  /** Rendered records, in order. */
  lines: LineRecord[];
  /** True if the requested range exceeds what the index currently covers; `lines` may be shorter. */
  partial: boolean;
  /** Set when the result is served from the tail buffer (line numbers are approximate). */
  fromTail?: boolean;
}

/**
 * Pre-rendered cache of the very last N lines of the file. Built once at
 * session start by reading the trailing bytes; serves window requests
 * past the indexed region so users can jump to the tail of a 10 GB file
 * in <500 ms (RFC §5.3).
 */
interface TailBuffer {
  lines: LineRecord[];
  /** Byte offset (in the source file) of the first line we kept. */
  byteStart: number;
}

/**
 * Pre-rendered cache of the first N lines of the file. Built once at session
 * start by reading the leading bytes, so the very first page renders
 * immediately — before the indexer has scanned far enough to produce any
 * anchors. Covers lines `[0, lines.length)` exactly (line index === array
 * index). Superseded by the index proper once anchors reach the head.
 */
interface HeadBuffer {
  lines: LineRecord[];
}

/** Default tail-read budget in bytes. */
export const DEFAULT_TAIL_BYTES = 1 * 1024 * 1024;
/** Default head-read budget in bytes (first-page fast path). */
export const DEFAULT_HEAD_BYTES = 1 * 1024 * 1024;
/** Default per-line size guess used to seed totalLines before scanning. */
export const DEFAULT_BYTES_PER_LINE_GUESS = 80;

export class StreamingSession {
  private readonly opts: StreamingSessionOptions;
  private readonly indexer: Indexer;
  private readonly anchors: bigint[] = [];
  private readonly windowCache = new WindowCache<LineRecord[]>(16);
  private readonly stride: number;
  private fd: number | null = null;
  private fileSize = 0;
  private mtimeMs = 0;
  private totalLines = 1; // updated on complete; pre-complete we expose density-estimated value
  private indexComplete = false;
  private disposed = false;
  private adjacentFallback = false;
  private tail: TailBuffer | null = null;
  private head: HeadBuffer | null = null;
  private fsWatcher: fs.FSWatcher | null = null;

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
      this.buildTailBuffer();
      this.startFileWatcher();
      this.opts.onIndexProgress({
        scannedLines: cached.header.totalLines - 1,
        scannedBytes: cached.header.fileSize,
        complete: true,
        totalLines: cached.header.totalLines,
        fileSize: this.fileSize,
      });
      return;
    }

    // Density-based seed so the scrollbar reaches the right ballpark
    // before the index has had a chance to scan more than a few MB.
    // Refined as anchors arrive.
    this.totalLines = Math.max(
      1,
      Math.floor(this.fileSize / DEFAULT_BYTES_PER_LINE_GUESS),
    );
    // Head/tail buffers: read the first and last ~1 MB right away so the
    // first page renders and "End" works immediately — before the indexer
    // has produced any anchors (RFC §5.3).
    this.buildHeadBuffer();
    this.buildTailBuffer();
    this.startFileWatcher();

    this.indexer.start({
      path: this.opts.fsPath,
      stride: this.stride,
      chunkSize: this.opts.chunkSize ?? DEFAULT_CHUNK_SIZE,
      batchAnchors: this.opts.batchAnchors ?? DEFAULT_BATCH_ANCHORS,
      workerScript: this.opts.workerScript,
      onProgress: ({ lines, bytes, newAnchors }) => {
        if (this.disposed) return;
        for (let i = 0; i < newAnchors.length; i++) this.anchors.push(newAnchors[i]);
        // Refine totalLines from measured density: extrapolate the
        // scanned head's bytes/line rate to the rest of the file. Clamp
        // below by `lines + 1` so it's never a downward surprise.
        if (bytes > 0 && lines > 0) {
          const bytesPerLine = bytes / lines;
          const remaining = Math.max(0, this.fileSize - bytes);
          const est = lines + 1 + Math.floor(remaining / bytesPerLine);
          this.totalLines = Math.max(est, lines + 1);
        } else {
          this.totalLines = Math.max(this.totalLines, lines + 1);
        }
        this.opts.onIndexProgress({
          scannedLines: lines,
          scannedBytes: bytes,
          complete: false,
          totalLines: this.totalLines,
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

  /**
   * Read the trailing slice of the file and render it via WASM so we can
   * serve "scroll to end" without waiting for the head-to-tail indexer
   * (RFC §5.3 — phase 6).
   *
   * Best-effort: errors are swallowed; the tail buffer just stays empty
   * and the user sees the indexing-in-progress placeholder until the
   * head scan catches up.
   */
  private buildTailBuffer(): void {
    if (this.fd === null) return;
    const budget = this.opts.tailBytes ?? DEFAULT_TAIL_BYTES;
    if (this.fileSize === 0 || budget <= 0) return;
    const byteStart = Math.max(0, this.fileSize - budget);
    let buf: Buffer;
    try {
      buf = this.readSlab(byteStart, this.fileSize);
    } catch {
      return;
    }
    if (buf.length === 0) return;
    // Skip the partial line at the head of the slab (it begins mid-line
    // unless we read from byte 0). The first '\n' marks where the next
    // whole line starts.
    let slabStart = 0;
    if (byteStart > 0) {
      const firstNl = buf.indexOf(0x0a);
      if (firstNl < 0) return; // no newline found in tail budget
      slabStart = firstNl + 1;
    }
    const slab = buf.subarray(slabStart);
    try {
      const json = this.opts.wasm.renderLines(new Uint8Array(slab));
      const parsed = JSON.parse(json) as ParsedLines;
      const lines: LineRecord[] = parsed.text.map((t, i) => ({
        text: t,
        html: parsed.html[i] ?? '',
      }));
      // The slab may end with '\n' (yielding a trailing empty record);
      // keep it so the tail aligns with totalLines (split semantics).
      this.tail = { lines, byteStart: byteStart + slabStart };
    } catch {
      // ignore render errors; tail just won't serve
    }
  }

  /**
   * Read the leading slice of the file and render it via WASM so the first
   * page is servable instantly, before the head-to-tail indexer has produced
   * any anchors (RFC §5.3 — first-page fast path).
   *
   * Best-effort: errors are swallowed; the head buffer just stays empty and
   * the first page shows the indexing-in-progress placeholder until anchors
   * arrive.
   */
  private buildHeadBuffer(): void {
    if (this.fd === null) return;
    const budget = this.opts.headBytes ?? DEFAULT_HEAD_BYTES;
    if (this.fileSize === 0 || budget <= 0) return;
    const readLen = Math.min(budget, this.fileSize);
    let buf: Buffer;
    try {
      buf = this.readSlab(0, readLen);
    } catch {
      return;
    }
    if (buf.length === 0) return;
    const reachedEof = readLen >= this.fileSize;
    let slab = buf;
    if (!reachedEof) {
      // The slab ends mid-line (we read a fixed byte budget). Trim back to the
      // last '\n' so we only keep complete lines; the partial tail line is
      // served later by the index proper.
      const lastNl = buf.lastIndexOf(0x0a);
      if (lastNl < 0) return; // first line longer than the budget — can't help
      slab = buf.subarray(0, lastNl + 1);
    }
    try {
      const json = this.opts.wasm.renderLines(new Uint8Array(slab));
      const parsed = JSON.parse(json) as ParsedLines;
      const lines: LineRecord[] = parsed.text.map((t, i) => ({
        text: t,
        html: parsed.html[i] ?? '',
      }));
      // A slab ending in '\n' yields a trailing empty record (split semantics).
      // When we trimmed mid-file, that record is the start of the next
      // (partial) line — drop it so `lines` maps 1:1 to whole lines [0, n).
      // When we read the whole file, keep every record so the head covers
      // [0, totalLines) including any genuine trailing empty line.
      if (!reachedEof && lines.length > 0 && lines[lines.length - 1].text === '') {
        lines.pop();
      }
      this.head = { lines };
    } catch {
      // ignore render errors; head just won't serve
    }
  }

  private startFileWatcher(): void {
    if (this.fsWatcher) return;
    try {
      this.fsWatcher = fs.watch(this.opts.fsPath, () => {
        if (this.disposed) return;
        try {
          const st = fs.statSync(this.opts.fsPath);
          // Only fire on real change to size or mtime to suppress the
          // chmod / inotify noise some filesystems emit.
          if (
            st.size !== this.fileSize ||
            Math.floor(st.mtimeMs) !== this.mtimeMs
          ) {
            this.opts.onFileChanged?.({
              previousSize: this.fileSize,
              currentSize: st.size,
              previousMtimeMs: this.mtimeMs,
              currentMtimeMs: Math.floor(st.mtimeMs),
            });
          }
        } catch {
          // file removed; let the host decide what to do
          this.opts.onFileChanged?.({
            previousSize: this.fileSize,
            currentSize: 0,
            previousMtimeMs: this.mtimeMs,
            currentMtimeMs: 0,
          });
        }
      });
    } catch {
      // some platforms (e.g. NFS) reject fs.watch; reload will need a manual nudge
    }
  }

  private tryLoadCache() {
    // Memory mode keeps the index in `this.anchors` only — nothing on disk.
    if (this.opts.cache.mode === 'memory') return null;
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
    // Memory mode never touches disk: skip the write and the eviction sweep.
    if (this.opts.cache.mode === 'memory') return;
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
      complete: this.indexComplete,
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
    if (start < 0 || start >= view.totalLines) {
      return { start, lines: [], partial: false };
    }

    const plan = planWindow(view, start, end);
    if (plan) {
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
      const partial = slice.length < end - start;
      // Only cache final results. A partial slice produced while indexing is
      // still in flight is provisional (the frontier hasn't reached `end`
      // yet); caching it would pin the gap and starve the re-request that
      // fills it once more anchors arrive. A partial slice on a complete
      // index is the genuine end of file and safe to cache.
      if (view.complete || !partial) {
        this.windowCache.set(cacheKey, slice);
      }
      return { start, lines: slice, partial };
    }

    // Not covered by the index yet. The head buffer covers the first page and
    // the tail buffer covers the end — try whichever the request falls in.
    const head = this.headFallback(start, end);
    if (head.lines.length > 0) return head;
    const tail = this.tailFallback(start, end);
    if (tail.lines.length > 0) return tail;
    return { start, lines: [], partial: true };
  }

  /**
   * Serve a window from the pre-rendered head buffer when the indexer hasn't
   * reached the requested range yet. The head buffer covers lines
   * `[0, head.lines.length)` exactly, so line numbers are precise (unlike the
   * tail, whose offset depends on the still-estimated `totalLines`).
   */
  private headFallback(start: number, end: number): WindowResult {
    const head = this.head;
    if (!head || head.lines.length === 0 || start >= head.lines.length) {
      return { start, lines: [], partial: true };
    }
    const localEnd = Math.min(head.lines.length, end);
    return {
      start,
      lines: head.lines.slice(start, localEnd),
      partial: localEnd - start < end - start,
    };
  }

  /**
   * Serve a window from the pre-rendered tail buffer when the indexer
   * hasn't reached the requested range. Line numbers shown to the user
   * are still relative to `totalLines` (which is an estimate until
   * indexing finishes), so the caller is told `fromTail=true`.
   */
  private tailFallback(start: number, end: number): WindowResult {
    const tail = this.tail;
    if (!tail || tail.lines.length === 0) {
      return { start, lines: [], partial: true };
    }
    const tailFirstLine = Math.max(0, this.totalLines - tail.lines.length);
    if (start < tailFirstLine) {
      // Requested window starts before the tail buffer covers — can't help.
      return { start, lines: [], partial: true };
    }
    const localStart = start - tailFirstLine;
    const localEnd = Math.min(tail.lines.length, end - tailFirstLine);
    if (localStart >= tail.lines.length) {
      return { start, lines: [], partial: true };
    }
    return {
      start,
      lines: tail.lines.slice(localStart, localEnd),
      partial: localEnd - localStart < end - start,
      fromTail: true,
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
    if (this.fsWatcher) {
      try {
        this.fsWatcher.close();
      } catch {
        // ignore
      }
      this.fsWatcher = null;
    }
    if (this.fd !== null) {
      try {
        fs.closeSync(this.fd);
      } catch {
        // ignore
      }
      this.fd = null;
    }
    this.windowCache.clear();
    this.tail = null;
    this.head = null;
  }
}
