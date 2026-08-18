import type { PDFDocumentProxy } from 'pdfjs-dist';

/**
 * PDF destinations — what an outline entry or an internal link points at.
 *
 * A destination is either a name to look up in the document's name tree or the
 * array itself, and the array's first element is a *reference* to a page
 * object, which only the document can turn into an index. The rest is a view
 * specification whose shape depends on its first word.
 */

/** A resolved destination: a page, and where on it to land. */
export interface Destination {
  /** 1-based. */
  page: number;
  /** A point in PDF user space, or null for a destination that names no edge. */
  point: { x: number; y: number } | null;
}

/**
 * The point a destination array names, in PDF user space.
 *
 * Only some of the view specifications say anything a scroller can use.
 * `/XYZ left top zoom` and the horizontal fits carry a top edge; `/Fit`,
 * `/FitV` and the rest describe a *fit*, not a position, and for those the top
 * of the page is the honest answer rather than a number invented from the
 * pieces that happen to be there.
 *
 * Split out from the lookup so the shape-reading can be tested without a
 * document — it is the half that goes quietly wrong.
 */
export function destinationPoint(
  array: readonly unknown[],
): { x: number; y: number } | null {
  const kind = (array[1] as { name?: string } | undefined)?.name;
  const point =
    kind === 'XYZ'
      ? { x: numberOr(array[2], 0), y: numberOr(array[3], Number.NaN) }
      : kind === 'FitH' || kind === 'FitBH'
        ? { x: 0, y: numberOr(array[2], Number.NaN) }
        : null;
  // `/XYZ null null null` is legal and means "keep the current view", which is
  // a page-top scroll here rather than a jump to NaN.
  return point && Number.isFinite(point.y) ? point : null;
}

/**
 * Resolve a destination against a document.
 *
 * Returns undefined for anything that does not resolve — a name the document
 * does not define, a reference to a page that is not there, a malformed array.
 * A link that goes nowhere does nothing, which beats throwing out of a click.
 */
export async function resolveDestination(
  doc: PDFDocumentProxy,
  dest: string | unknown[] | null | undefined,
): Promise<Destination | undefined> {
  if (dest === null || dest === undefined) return undefined;
  try {
    const array = typeof dest === 'string' ? await doc.getDestination(dest) : dest;
    if (!Array.isArray(array) || array.length === 0) return undefined;
    const index = await doc.getPageIndex(array[0] as Parameters<typeof doc.getPageIndex>[0]);
    return { page: index + 1, point: destinationPoint(array) };
  } catch {
    return undefined;
  }
}

function numberOr(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}
