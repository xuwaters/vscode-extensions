// Pure scan algorithm: walk a file's bytes, find every '\n', and emit
// sparse anchors at every Nth line.
//
// Factored out of the worker so it can be unit-tested without spawning a
// worker thread. The worker entry (`worker.ts`) wires this up to fs.readSync;
// tests wire it up to an in-memory Buffer.

export interface ScanOptions {
  /** Anchor every `stride` lines. */
  stride: number;
  /** Bytes per read. */
  chunkSize: number;
  /**
   * Synchronously fill `buf[0..len)` from `offset` of the source and return
   * the number of bytes actually read. Return 0 to signal EOF.
   */
  read: (offset: bigint, buf: Buffer, len: number) => number;
  /**
   * Called when a batch of anchors is ready to flush to the consumer.
   * `byteOffset` is the byte position the worker has scanned through;
   * `lineCount` is the number of newlines seen so far. Anchors are passed
   * incrementally — the consumer concatenates them.
   */
  emitAnchors: (anchors: BigUint64Array, lineCount: number, byteOffset: bigint) => void;
  /** Polled between chunks; if true, scan aborts at the next chunk boundary. */
  isCancelled?: () => boolean;
  /** Flush threshold for pending anchors (default: 4096). */
  batchAnchors?: number;
  /**
   * Flush pending anchors (and a progress event) at least every this many
   * bytes of scanned data, even if `batchAnchors` hasn't been reached
   * (default: `chunkSize`). Keeps the index progressively usable and the
   * indexing progress bar moving from the first chunk rather than only
   * after ~`batchAnchors * stride` lines have been seen.
   */
  flushIntervalBytes?: number;
}

export interface ScanResult {
  /** Total number of lines in the file (including a trailing empty line if the file ends with '\n'). */
  totalLines: number;
  /** Total bytes scanned. */
  fileSize: bigint;
  /** True iff scan exited early due to cancellation. */
  cancelled: boolean;
}

/**
 * Scan the source for newlines and emit sparse anchors.
 *
 * Line numbering: line 0 begins at byte 0; every '\n' starts a new line.
 * `totalLines = newlines + 1`. Anchors record the byte offset where every
 * `stride`th line begins (so `anchors[0] = 0n` always).
 */
export function scanIndex(opts: ScanOptions): ScanResult {
  const stride = opts.stride;
  if (stride < 1) throw new Error('scanIndex: stride must be >= 1');
  const chunkSize = opts.chunkSize;
  if (chunkSize < 1) throw new Error('scanIndex: chunkSize must be >= 1');
  const batchAnchors = opts.batchAnchors ?? 4096;
  const flushIntervalBytes = BigInt(opts.flushIntervalBytes ?? chunkSize);

  const buf = Buffer.allocUnsafe(chunkSize);
  let fileOffset = 0n;
  let lineNum = 0; // count of newlines seen
  // We pre-emit anchor[0] for line 0 = byte 0.
  let pending: bigint[] = [0n];
  let bytesAtLastFlush = 0n;

  const flush = (final: boolean): void => {
    if (pending.length === 0) return;
    const arr = new BigUint64Array(pending.length);
    for (let i = 0; i < pending.length; i++) arr[i] = pending[i];
    opts.emitAnchors(arr, lineNum, fileOffset);
    pending = [];
    bytesAtLastFlush = fileOffset;
    void final;
  };

  while (true) {
    if (opts.isCancelled?.()) {
      flush(true);
      return { totalLines: lineNum + 1, fileSize: fileOffset, cancelled: true };
    }
    const n = opts.read(fileOffset, buf, chunkSize);
    if (n <= 0) break;
    let i = 0;
    while (i < n) {
      const j = buf.indexOf(0x0a, i);
      if (j === -1 || j >= n) break;
      lineNum += 1;
      if (lineNum % stride === 0) {
        // Next line begins immediately after this '\n'.
        pending.push(fileOffset + BigInt(j + 1));
      }
      i = j + 1;
    }
    fileOffset += BigInt(n);
    if (
      pending.length >= batchAnchors ||
      (pending.length > 0 && fileOffset - bytesAtLastFlush >= flushIntervalBytes)
    ) {
      flush(false);
    }
  }
  flush(true);
  return { totalLines: lineNum + 1, fileSize: fileOffset, cancelled: false };
}
