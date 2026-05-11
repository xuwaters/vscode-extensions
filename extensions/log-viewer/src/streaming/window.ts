// Window slicing: given the index and a line range, compute the byte range
// to read and the local line offsets within the read.

export interface WindowPlan {
  /** Byte to seek to. */
  byteStart: number;
  /** Bytes after the last we need. May exceed fileSize; clamp before reading. */
  byteEnd: number;
  /** Line number of the first line contained in `[byteStart, byteEnd)`. */
  firstLineInSlab: number;
  /** How many lines the slab nominally contains (parsed text may end with an extra empty entry; truncate). */
  linesInSlab: number;
  /** Local index of `start` within the slab. */
  localStart: number;
  /** Local index (exclusive) of `end` within the slab. */
  localEnd: number;
}

export interface IndexView {
  anchors: BigUint64Array;
  stride: number;
  totalLines: number;
  fileSize: number;
}

/**
 * Plan the read needed to display lines `[start, end)`. Returns `null` if
 * the requested range isn't covered yet (the index hasn't reached that
 * far).
 */
export function planWindow(idx: IndexView, start: number, end: number): WindowPlan | null {
  if (end <= start) return null;
  const total = idx.totalLines;
  if (start < 0 || start >= total) return null;
  const clampedEnd = Math.min(end, total);
  const stride = idx.stride;

  const startAnchorIdx = Math.floor(start / stride);
  // We need an anchor (or EOF) for line `clampedEnd` so the slab ends at a
  // known byte boundary. ceil(end/stride) yields the next anchor past end-1.
  const endAnchorIdx = Math.ceil(clampedEnd / stride);

  if (startAnchorIdx >= idx.anchors.length) return null;
  const byteStart = Number(idx.anchors[startAnchorIdx]);
  const firstLineInSlab = startAnchorIdx * stride;

  let byteEnd: number;
  let linesInSlab: number;
  if (endAnchorIdx < idx.anchors.length) {
    byteEnd = Number(idx.anchors[endAnchorIdx]);
    linesInSlab = endAnchorIdx * stride - firstLineInSlab;
  } else {
    // Beyond the last anchor: only safe to read if the index is at EOF.
    // Caller can detect partial coverage by checking the returned plan
    // against its known progress.
    byteEnd = idx.fileSize;
    linesInSlab = total - firstLineInSlab;
  }
  const localStart = start - firstLineInSlab;
  const localEnd = clampedEnd - firstLineInSlab;
  return {
    byteStart,
    byteEnd,
    firstLineInSlab,
    linesInSlab,
    localStart,
    localEnd,
  };
}

/**
 * Simple FIFO/LRU cache for rendered windows. Keyed by the line range
 * `${start}:${end}`. Capacity is the number of entries, not bytes.
 */
export class WindowCache<T> {
  private readonly map = new Map<string, T>();
  constructor(private readonly capacity: number) {}

  get(key: string): T | undefined {
    const v = this.map.get(key);
    if (v !== undefined) {
      // refresh recency
      this.map.delete(key);
      this.map.set(key, v);
    }
    return v;
  }

  set(key: string, value: T): void {
    if (this.map.has(key)) this.map.delete(key);
    this.map.set(key, value);
    while (this.map.size > this.capacity) {
      const oldest = this.map.keys().next().value;
      if (oldest === undefined) break;
      this.map.delete(oldest);
    }
  }

  clear(): void {
    this.map.clear();
  }
}
