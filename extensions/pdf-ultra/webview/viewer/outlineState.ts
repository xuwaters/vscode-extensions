import { observable } from '@microsoft/fast-element';
import {
  flattenOutline,
  rowForPage,
  visibleRows,
  type OutlineRow,
  type RawOutlineItem,
} from '../model/outline.js';

/**
 * The outline sidebar's state.
 *
 * `model/outline.ts` holds the rules — how a tree flattens, what a collapsed
 * row hides, which row a page belongs under. This holds the *state* those rules
 * are applied to, in the two observables the sidebar binds to.
 *
 * `collapsed` is replaced rather than mutated on every change. That is not a
 * style choice: a FAST binding only re-evaluates when something it *read*
 * notifies, and a `Set` that is added to in place notifies nobody — the twisty
 * on the row you just collapsed would keep pointing down.
 */
export class OutlineState {
  /** The rows to render, with collapsed subtrees removed. */
  @observable shown: OutlineRow[] = [];
  /** The id of the row the reader is inside, or ''. */
  @observable current = '';
  /** The ids whose subtrees are hidden. */
  @observable collapsed: ReadonlySet<string> = new Set();

  private rows: OutlineRow[] = [];
  /** The page each row points at, filled in as destinations resolve. */
  private pages: (number | undefined)[] = [];

  get all(): readonly OutlineRow[] {
    return this.rows;
  }

  /** Take a document's outline, or its lack of one. */
  load(raw: readonly RawOutlineItem[] | null | undefined): void {
    this.rows = flattenOutline(raw);
    this.pages = new Array<number | undefined>(this.rows.length).fill(undefined);
    this.collapsed = new Set();
    this.current = '';
    this.refresh();
  }

  isCollapsed(row: OutlineRow): boolean {
    return this.collapsed.has(row.id);
  }

  toggle(row: OutlineRow): void {
    this.setCollapsed(row, !this.collapsed.has(row.id));
  }

  setCollapsed(row: OutlineRow, collapsed: boolean): void {
    if (this.collapsed.has(row.id) === collapsed) return;
    const next = new Set(this.collapsed);
    if (collapsed) next.add(row.id);
    else next.delete(row.id);
    this.collapsed = next;
    this.refresh();
  }

  /** Record where a row's destination turned out to point. */
  setPage(index: number, page: number | undefined): void {
    this.pages[index] = page;
  }

  /** Mark the row the reader is inside, given the page they are on. */
  markPage(page: number): void {
    const at = rowForPage(this.pages, page);
    this.current = at >= 0 ? (this.rows[at]?.id ?? '') : '';
  }

  private refresh(): void {
    this.shown = visibleRows(this.rows, this.collapsed);
  }
}
