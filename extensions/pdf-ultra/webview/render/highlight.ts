/**
 * Find highlighting, through the CSS Custom Highlight API.
 *
 * The alternative is to wrap matches in `<mark>` elements, which means editing
 * the text layer pdf.js just built — and the text layer's geometry is inline
 * styles on one span per run, so splitting a run breaks its positioning and the
 * selection that reads across it. A highlight is painted over the text without
 * touching the DOM at all, which is exactly the difference.
 *
 * Where the API is missing the viewer still finds and still scrolls; it just
 * does not paint. That is a degradation worth having over a text layer that
 * moves when you search it.
 */

/** The registry names, which `::highlight()` in the stylesheet has to match. */
export const ALL = 'pdf-find';
export const CURRENT = 'pdf-find-current';

export function supported(): boolean {
  return typeof CSS !== 'undefined' && 'highlights' in CSS && typeof Highlight === 'function';
}

/**
 * A range over part of one text run.
 *
 * pdf.js writes a run's characters into a single text node, so the offsets from
 * `sliceMatch` address it directly. A run rendered as something else — an empty
 * run, or one pdf.js decorated — yields nothing rather than a wrong range.
 */
export function rangeIn(div: HTMLElement, start: number, end: number): Range | null {
  const node = div.firstChild;
  if (!node || node.nodeType !== Node.TEXT_NODE) return null;
  const length = node.textContent?.length ?? 0;
  if (start >= length || end > length || end <= start) return null;
  const range = document.createRange();
  range.setStart(node, start);
  range.setEnd(node, end);
  return range;
}

/** Paint these ranges, with `current` on top. Replaces whatever was painted. */
export function paint(all: readonly Range[], current: readonly Range[]): void {
  if (!supported()) return;
  CSS.highlights.set(ALL, new Highlight(...all));
  CSS.highlights.set(CURRENT, new Highlight(...current));
}

export function clear(): void {
  if (!supported()) return;
  CSS.highlights.delete(ALL);
  CSS.highlights.delete(CURRENT);
}
