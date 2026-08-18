/**
 * The document's outline, flattened into rows.
 *
 * pdf.js hands back a tree; a sidebar renders a list. Flattening it here rather
 * than recursing in the template keeps the FAST `repeat` over one array — and
 * makes "which rows are hidden because their parent is collapsed" a property of
 * the data, testable without a DOM.
 */

/** One entry as pdf.js reports it. Only the parts a sidebar needs are named. */
export interface RawOutlineItem {
  title: string;
  /** A named destination, or the destination array itself. */
  dest?: string | unknown[] | null;
  /** Set instead of `dest` for an entry that points out of the document. */
  url?: string | null;
  bold?: boolean;
  italic?: boolean;
  items?: RawOutlineItem[];
}

/** One row of the sidebar. */
export interface OutlineRow {
  /** Stable within one document; the row's identity for FAST's `repeat`. */
  id: string;
  title: string;
  depth: number;
  dest?: string | unknown[] | null;
  url?: string | null;
  bold: boolean;
  italic: boolean;
  /** Ids of this row's descendants, so collapsing one hides all of them. */
  children: string[];
  hasChildren: boolean;
  parent?: string;
}

/** How deep the sidebar will go before it stops recursing. */
export const MAX_OUTLINE_DEPTH = 12;

/**
 * Flatten the tree, depth-first, in reading order.
 *
 * Bounded on both axes. A document is an untrusted input and an outline is a
 * graph the format does not promise is a tree: a cycle would otherwise be an
 * infinite sidebar, and a 200 000-entry outline a locked-up tab.
 */
export function flattenOutline(
  items: readonly RawOutlineItem[] | null | undefined,
  limit = 5000,
): OutlineRow[] {
  const rows: OutlineRow[] = [];
  const seen = new Set<RawOutlineItem>();

  /** Returns every id in this subtree, so a parent can hold all its descendants. */
  const walk = (
    list: readonly RawOutlineItem[],
    depth: number,
    parent: string | undefined,
  ): string[] => {
    const subtree: string[] = [];
    if (depth > MAX_OUTLINE_DEPTH) return subtree;
    for (const item of list) {
      if (rows.length >= limit) break;
      if (seen.has(item)) continue;
      seen.add(item);

      const id = `o${rows.length}`;
      const row: OutlineRow = {
        id,
        title: (item.title ?? '').replace(/\s+/g, ' ').trim() || 'Untitled',
        depth,
        dest: item.dest ?? null,
        url: item.url ?? null,
        bold: item.bold === true,
        italic: item.italic === true,
        children: [],
        hasChildren: (item.items?.length ?? 0) > 0,
        parent,
      };
      rows.push(row);
      subtree.push(id);

      if (item.items?.length) {
        // A collapsed row hides its whole subtree, not just its first level.
        row.children = walk(item.items, depth + 1, id);
        subtree.push(...row.children);
      }
    }
    return subtree;
  };

  walk(items ?? [], 0, undefined);
  return rows;
}

/** The rows a sidebar shows, given which ones the reader has collapsed. */
export function visibleRows(
  rows: readonly OutlineRow[],
  collapsed: ReadonlySet<string>,
): OutlineRow[] {
  if (collapsed.size === 0) return [...rows];
  const hidden = new Set<string>();
  for (const row of rows) {
    if (collapsed.has(row.id)) for (const child of row.children) hidden.add(child);
  }
  return rows.filter((row) => !hidden.has(row.id));
}

/**
 * The row to mark as current for a page — the last one whose destination is at
 * or before it. `-1` when the reader is above the first entry.
 */
export function rowForPage(pages: readonly (number | undefined)[], page: number): number {
  let best = -1;
  pages.forEach((at, index) => {
    if (at !== undefined && at <= page) best = index;
  });
  return best;
}
