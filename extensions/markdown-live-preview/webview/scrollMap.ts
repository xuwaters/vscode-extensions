/**
 * Line ↔ scroll-offset mapping built from `data-sourcepos` anchors.
 *
 * The map is a sorted list of (0-based source line, absolute document offset)
 * pairs. Unmapped lines interpolate linearly between anchors; both lookup
 * directions use binary search. The pure functions operate on plain arrays so
 * they are unit-testable without a DOM.
 */

export interface MapEntry {
  /** 0-based source line. */
  line: number;
  /** Absolute document Y offset of the element's top edge. */
  top: number;
}

/** Parse the start line of a `data-sourcepos="12:1-14:8"` value; 0-based. */
export function sourceposLine(value: string | null): number | null {
  if (!value) return null;
  const line = Number.parseInt(value, 10);
  return Number.isFinite(line) && line >= 1 ? line - 1 : null;
}

/** Build the map from every sourcepos anchor under `root`, in DOM order. */
export function buildScrollMap(root: HTMLElement): MapEntry[] {
  const entries: MapEntry[] = [];
  const els = root.querySelectorAll<HTMLElement>('[data-sourcepos]');
  for (const el of els) {
    const line = sourceposLine(el.getAttribute('data-sourcepos'));
    if (line === null) continue;
    const top = el.getBoundingClientRect().top + window.scrollY;
    entries.push({ line, top });
  }
  return normalize(entries);
}

/** Sort by line, drop out-of-order offsets and duplicate lines. */
export function normalize(entries: MapEntry[]): MapEntry[] {
  const sorted = [...entries].sort((a, b) => a.line - b.line || a.top - b.top);
  const out: MapEntry[] = [];
  for (const e of sorted) {
    const last = out[out.length - 1];
    if (last && e.line === last.line) continue;
    if (last && e.top < last.top) continue;
    out.push(e);
  }
  return out;
}

/** Scroll offset that puts source `line` at the top of the viewport. */
export function offsetForLine(map: MapEntry[], line: number): number | null {
  if (map.length === 0) return null;
  if (line <= map[0].line) return map[0].top;
  const last = map[map.length - 1];
  if (line >= last.line) return last.top;

  const i = upperBound(map, line);
  const before = map[i - 1];
  const after = map[i];
  if (after.line === before.line) return before.top;
  const progress = (line - before.line) / (after.line - before.line);
  return before.top + progress * (after.top - before.top);
}

/** Source line whose block sits at scroll offset `top` (interpolated). */
export function lineForOffset(map: MapEntry[], top: number): number | null {
  if (map.length === 0) return null;
  if (top <= map[0].top) return map[0].line;
  const last = map[map.length - 1];
  if (top >= last.top) return last.line;

  let lo = 0;
  let hi = map.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (map[mid].top <= top) lo = mid;
    else hi = mid - 1;
  }
  const before = map[lo];
  const after = map[lo + 1];
  if (!after || after.top === before.top) return before.line;
  const progress = (top - before.top) / (after.top - before.top);
  return Math.round(before.line + progress * (after.line - before.line));
}

/** First index whose line is strictly greater than `line`. */
function upperBound(map: MapEntry[], line: number): number {
  let lo = 0;
  let hi = map.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (map[mid].line <= line) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}
