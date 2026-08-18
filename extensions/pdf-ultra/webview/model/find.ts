/**
 * Find-in-document, as arithmetic over extracted text.
 *
 * Everything here is pure: a page's text is a string, a match is an offset into
 * it, and turning that offset back into something on screen is the only part
 * that needs the DOM. Which is what makes the search itself testable — the
 * mapping from "the 4th match" to "this span, these characters" is exactly the
 * part that goes quietly wrong.
 */

/**
 * One text run as pdf.js emits it. `eol` marks the end of a line: the drawn
 * text carries no newline, so the joined text has to put one back or a search
 * for "the end" would match across a line break that reads as a space.
 */
export interface PageItem {
  text: string;
  eol: boolean;
}

export type PageItems = readonly PageItem[];

/** A match, located in the page's joined text. */
export interface Match {
  /** 1-based. */
  page: number;
  /** Offset into the page's joined text. */
  start: number;
  length: number;
}

/**
 * Where one text run sits in the page's joined text.
 *
 * The range covers the run's own characters only — the newline a line-ending
 * run contributes belongs to no run, because no text node on the page holds it.
 */
export interface ItemSpan {
  index: number;
  start: number;
  end: number;
}

/**
 * The joined text of a page, and the map back to the runs it came from.
 *
 * Runs are joined with nothing between them beyond those line breaks: pdf.js
 * already emits the spacing a document draws, and inserting more would make
 * "in voice" out of "invoice" and hide a match the reader can plainly see.
 */
export function joinItems(items: PageItems): { text: string; spans: ItemSpan[] } {
  const spans: ItemSpan[] = [];
  let text = '';
  items.forEach((item, index) => {
    const start = text.length;
    text += item.text;
    spans.push({ index, start, end: text.length });
    if (item.eol) text += '\n';
  });
  return { text, spans };
}

/**
 * Fold a page's text for comparison: case, and the several kinds of space a
 * document's text layer uses where a reader typed one.
 *
 * Length-preserving on purpose — every offset into the folded text is an offset
 * into the original, so a match found here can be highlighted there.
 */
export function fold(text: string): string {
  return text.toLowerCase().replace(/\s/g, ' ');
}

/** Every match of `needle` in one page's text, left to right, non-overlapping. */
export function matchesInPage(page: number, text: string, needle: string): Match[] {
  const folded = fold(text);
  const target = fold(needle);
  if (target.length === 0) return [];

  const found: Match[] = [];
  let at = folded.indexOf(target);
  while (at !== -1) {
    found.push({ page, start: at, length: target.length });
    at = folded.indexOf(target, at + target.length);
  }
  return found;
}

/**
 * The match to move to from where the reader stands.
 *
 * Wraps in both directions, because a search that stops at the last match makes
 * the reader guess whether there were none above. `-1` for an empty list.
 */
export function stepMatch(total: number, current: number, direction: 1 | -1): number {
  if (total <= 0) return -1;
  if (current < 0) return direction > 0 ? 0 : total - 1;
  return (current + direction + total) % total;
}

/**
 * The first match at or after `page` — where a search started mid-document
 * should land, rather than back at the top.
 */
export function matchNear(matches: readonly Match[], page: number): number {
  if (matches.length === 0) return -1;
  const at = matches.findIndex((match) => match.page >= page);
  return at === -1 ? 0 : at;
}

/** A slice of a match that falls inside one text run. */
export interface ItemSlice {
  index: number;
  start: number;
  end: number;
}

/**
 * Cut a match into the text runs it crosses.
 *
 * A phrase rarely lives in one run: pdf.js emits one per change of font,
 * position or kerning, so "Content-Security-Policy" can be four of them. Each
 * slice names a run and the character range within it, which is exactly what a
 * `Range` over that run's text node needs.
 */
export function sliceMatch(spans: readonly ItemSpan[], match: Match): ItemSlice[] {
  const end = match.start + match.length;
  const slices: ItemSlice[] = [];
  for (const span of spans) {
    if (span.end <= match.start) continue;
    if (span.start >= end) break;
    const from = Math.max(span.start, match.start) - span.start;
    const to = Math.min(span.end, end) - span.start;
    if (to > from) slices.push({ index: span.index, start: from, end: to });
  }
  return slices;
}

/** How the toolbar reads out the state of a search. */
export function findLabel(query: string, total: number, current: number): string {
  if (query.trim() === '') return '';
  if (total === 0) return 'No results';
  return `${current + 1} of ${total}`;
}
