/**
 * Where every row and column sits, and which of them are on screen.
 *
 * The arithmetic behind the whole grid, and the reason a million-row file scrolls
 * at all: nothing is laid out by the browser. The body is one box the size of the
 * whole table with a few hundred absolutely-positioned cells inside it, and this
 * is what says where those cells go and which ones they should be.
 *
 * Columns and rows are kept differently on purpose, because there are thousands
 * of one and millions of the other:
 *
 * * A column's offset is a dense prefix sum. There are never enough columns for
 *   that to matter, and it makes every lookup a subscript.
 * * A row's offset is arithmetic — `index × height` — corrected by the handful of
 *   rows that have been dragged to a size of their own. A dense prefix sum over
 *   two million rows would be 16 MB of numbers rebuilt on every drag.
 */

/** The visible slice of an axis. `last` is inclusive; `last < first` means none. */
export interface Window {
  first: number;
  last: number;
}

const EMPTY: Window = { first: 0, last: -1 };

/**
 * Column widths.
 *
 * A width is remembered per column and rebuilt as one prefix array, because the
 * column headers, the cells, the drag handles and the horizontal scroll extent
 * all need to agree about where column 40 starts down to the pixel — and the
 * cheapest way to be sure of that is for all four to ask the same array.
 */
export class ColumnMetrics {
  private readonly widths = new Map<number, number>();
  private offsets: number[] = [0];
  private columns = 0;

  constructor(
    private fallback: number,
    readonly min = 32,
    readonly max = 2000,
  ) {}

  get count(): number {
    return this.columns;
  }

  setCount(count: number): void {
    if (count === this.columns) return;
    this.columns = Math.max(0, count);
    this.rebuild();
  }

  /** The width every unsized column takes. */
  setDefault(width: number): void {
    const clamped = this.clamp(width);
    if (clamped === this.fallback) return;
    this.fallback = clamped;
    this.rebuild();
  }

  width(column: number): number {
    return this.widths.get(column) ?? this.fallback;
  }

  setWidth(column: number, width: number): void {
    if (column < 0) return;
    this.widths.set(column, this.clamp(width));
    this.rebuild();
  }

  /**
   * Give several columns a width each, in one pass.
   *
   * Sizing a selection one column at a time would rebuild the prefix array once
   * per column, which is quadratic in a number the reader picks — and picking
   * every column is one keystroke.
   */
  setWidths(entries: Iterable<readonly [number, number]>): void {
    let changed = false;
    for (const [column, width] of entries) {
      if (column < 0) continue;
      this.widths.set(column, this.clamp(width));
      changed = true;
    }
    if (changed) this.rebuild();
  }

  /** Whether this column has a width of its own, rather than the default. */
  isSized(column: number): boolean {
    return this.widths.has(column);
  }

  /** Forget every dragged width. */
  clear(): void {
    if (this.widths.size === 0) return;
    this.widths.clear();
    this.rebuild();
  }

  offset(column: number): number {
    if (column <= 0) return 0;
    if (column >= this.offsets.length) return this.total();
    return this.offsets[column]!;
  }

  total(): number {
    return this.offsets[this.offsets.length - 1] ?? 0;
  }

  /** Which column an x-offset falls in, clamped to the table. */
  indexAt(x: number): number {
    if (this.columns === 0) return 0;
    if (x <= 0) return 0;
    // The prefix array is sorted, so this is a plain upper bound.
    let low = 0;
    let high = this.columns - 1;
    let found = this.columns - 1;
    while (low <= high) {
      const middle = (low + high) >> 1;
      if (this.offsets[middle]! <= x) {
        found = middle;
        low = middle + 1;
      } else {
        high = middle - 1;
      }
    }
    return found;
  }

  window(from: number, to: number): Window {
    if (this.columns === 0) return EMPTY;
    return { first: this.indexAt(from), last: this.indexAt(to) };
  }

  /** The dragged widths, for the layout memory. */
  sizes(): Array<[number, number]> {
    return [...this.widths].sort((a, b) => a[0] - b[0]);
  }

  restore(pairs: ReadonlyArray<readonly [number, number]>): void {
    this.widths.clear();
    for (const [column, width] of pairs) {
      if (column >= 0) this.widths.set(column, this.clamp(width));
    }
    this.rebuild();
  }

  private clamp(width: number): number {
    if (!Number.isFinite(width)) return this.fallback;
    return Math.round(Math.min(this.max, Math.max(this.min, width)));
  }

  private rebuild(): void {
    const offsets = new Array<number>(this.columns + 1);
    offsets[0] = 0;
    for (let column = 0; column < this.columns; column += 1) {
      offsets[column + 1] = offsets[column]! + this.width(column);
    }
    this.offsets = offsets;
  }
}

/**
 * Row heights: one default, and the few rows that differ.
 *
 * `offset(i)` is `i × height` plus the accumulated difference of every sized row
 * above it, which a binary search over the sized rows answers in a few steps
 * however many rows there are. `indexAt(y)` is the same idea run backwards, and
 * it is the one that has to be exactly right: it is called on every scroll frame
 * with the scroll offset, and an answer one row out is a table that jumps.
 */
export class RowMetrics {
  private readonly heights = new Map<number, number>();
  /** Sized rows, ascending. */
  private keys: number[] = [];
  /** `deltas[j]` is the total extra height of `keys[0…j-1]`, so it has one more entry. */
  private deltas: number[] = [0];
  private rows = 0;

  constructor(
    private fallback: number,
    readonly min = 12,
    readonly max = 800,
  ) {}

  get count(): number {
    return this.rows;
  }

  /** The height an unsized row takes. */
  get defaultHeight(): number {
    return this.fallback;
  }

  setCount(count: number): void {
    const rows = Math.max(0, count);
    if (rows === this.rows) return;
    this.rows = rows;
    // A file that shrank leaves sized rows past its end. They stay in the map —
    // an undo brings the rows back and the reader expects their heights back
    // with them — but they must not be counted into the table's height.
    this.rebuild();
  }

  setDefault(height: number): void {
    const clamped = this.clamp(height);
    if (clamped === this.fallback) return;
    this.fallback = clamped;
    this.rebuild();
  }

  height(row: number): number {
    return this.heights.get(row) ?? this.fallback;
  }

  setHeight(row: number, height: number): void {
    if (row < 0) return;
    this.heights.set(row, this.clamp(height));
    this.rebuild();
  }

  /** Give several rows a height each, in one pass — see `ColumnMetrics.setWidths`. */
  setHeights(entries: Iterable<readonly [number, number]>): void {
    let changed = false;
    for (const [row, height] of entries) {
      if (row < 0) continue;
      this.heights.set(row, this.clamp(height));
      changed = true;
    }
    if (changed) this.rebuild();
  }

  isSized(row: number): boolean {
    return this.heights.has(row);
  }

  /** Give a row the default height back. */
  reset(row: number): void {
    if (!this.heights.delete(row)) return;
    this.rebuild();
  }

  clear(): void {
    if (this.heights.size === 0) return;
    this.heights.clear();
    this.rebuild();
  }

  offset(row: number): number {
    const index = Math.max(0, Math.min(row, this.rows));
    return index * this.fallback + this.deltas[this.before(index)]!;
  }

  total(): number {
    return this.rows * this.fallback + this.deltas[this.keys.length]!;
  }

  /** Which row a y-offset falls in, clamped to the table. */
  indexAt(y: number): number {
    if (this.rows === 0) return 0;
    const target = Math.max(0, y);

    // The last sized row that starts at or before `target`.
    let low = 0;
    let high = this.keys.length - 1;
    let found = -1;
    while (low <= high) {
      const middle = (low + high) >> 1;
      if (this.keys[middle]! * this.fallback + this.deltas[middle]! <= target) {
        found = middle;
        low = middle + 1;
      } else {
        high = middle - 1;
      }
    }

    if (found < 0) {
      return Math.min(this.rows - 1, Math.floor(target / this.fallback));
    }

    const key = this.keys[found]!;
    const base = key * this.fallback + this.deltas[found]!;
    const size = this.height(key);
    if (target < base + size) return key;
    // Everything between this sized row and the next is the default height, so
    // the rest is division. The search guarantees the answer lands before the
    // next sized row.
    const after = key + 1 + Math.floor((target - base - size) / this.fallback);
    return Math.min(this.rows - 1, after);
  }

  window(from: number, to: number): Window {
    if (this.rows === 0) return EMPTY;
    return { first: this.indexAt(from), last: this.indexAt(to) };
  }

  sizes(): Array<[number, number]> {
    return [...this.heights].sort((a, b) => a[0] - b[0]);
  }

  restore(pairs: ReadonlyArray<readonly [number, number]>): void {
    this.heights.clear();
    for (const [row, height] of pairs) {
      if (row >= 0) this.heights.set(row, this.clamp(height));
    }
    this.rebuild();
  }

  private clamp(height: number): number {
    if (!Number.isFinite(height)) return this.fallback;
    return Math.round(Math.min(this.max, Math.max(this.min, height)));
  }

  /** How many sized rows come before `row`. */
  private before(row: number): number {
    let low = 0;
    let high = this.keys.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if (this.keys[middle]! < row) low = middle + 1;
      else high = middle;
    }
    return low;
  }

  private rebuild(): void {
    this.keys = [...this.heights.keys()]
      .filter((row) => row < this.rows)
      .sort((a, b) => a - b);
    const deltas = new Array<number>(this.keys.length + 1);
    deltas[0] = 0;
    for (let index = 0; index < this.keys.length; index += 1) {
      deltas[index + 1] = deltas[index]! + (this.height(this.keys[index]!) - this.fallback);
    }
    this.deltas = deltas;
  }
}
