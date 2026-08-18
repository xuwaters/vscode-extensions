import { observable } from '@microsoft/fast-element';
import {
  findLabel,
  joinItems,
  matchNear,
  matchesInPage,
  sliceMatch,
  stepMatch,
  type Match,
} from '../model/find.js';
import { clear as clearHighlights, paint, rangeIn } from '../render/highlight.js';
import type { PageColumn } from '../render/pageColumn.js';

/** How long the page waits for typing to stop before it searches. */
export const FIND_DEBOUNCE_MS = 220;

/** What the search needs from the viewer around it. */
export interface SearchContext {
  /** The column to search and paint into, or null before a document is open. */
  column(): PageColumn | null;
  pageCount(): number;
  /** The page being read, so a search starts from there rather than the top. */
  page(): number;
  /** Bring a match on screen. */
  reveal(range: Range): void;
}

/**
 * Find-in-document, as a controller the element delegates to.
 *
 * Its three observables are what the toolbar binds to; everything else — the
 * match list, the scan, the debounce, the painting — is its own business. It is
 * a plain class rather than part of the element because it is the one part of
 * the viewer with a lifecycle of its own: a scan runs across many awaits and
 * has to be abandonable at any of them.
 */
export class Search {
  @observable query = '';
  @observable total = 0;
  @observable label = '';

  private matches: Match[] = [];
  private at = -1;
  /** Bumped to abandon a scan that a newer query has superseded. */
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly context: SearchContext) {}

  /** Whether a scan is pending, so Enter can run it now instead of stepping. */
  get pending(): boolean {
    return this.timer !== undefined;
  }

  /** The reader typed. Debounced: a scan per keystroke would read the document per keystroke. */
  type(value: string): void {
    this.query = value;
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.run();
    }, FIND_DEBOUNCE_MS);
  }

  /** Search now, cancelling any pending debounce. */
  async run(): Promise<void> {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = undefined;
    }
    const column = this.context.column();
    const query = this.query.trim();
    const generation = ++this.generation;

    if (!column || query === '') {
      this.reset();
      return;
    }

    // Page by page, awaiting each extraction, so a 900-page document does not
    // queue 900 concurrent text requests at the worker — and so a keystroke
    // arriving mid-scan can abandon the scan at the next await.
    const found: Match[] = [];
    const pages = this.context.pageCount();
    for (let page = 1; page <= pages; page += 1) {
      const items = await column.textItems(page);
      if (generation !== this.generation) return;
      found.push(...matchesInPage(page, joinItems(items).text, query));
    }

    this.matches = found;
    this.total = found.length;
    this.at = matchNear(found, this.context.page());
    this.describe();
    await this.show();
  }

  /** Move to the next or previous match, wrapping at either end. */
  async step(direction: 1 | -1): Promise<void> {
    if (this.matches.length === 0) {
      await this.run();
      return;
    }
    this.at = stepMatch(this.matches.length, this.at, direction);
    this.describe();
    await this.show();
  }

  /** Escape in the find box: stop searching and unpaint. */
  clear(): void {
    this.query = '';
    void this.run();
  }

  /**
   * Repaint every match that currently has a text layer under it.
   *
   * Called whenever a page's text layer is rebuilt, because a scroll or a zoom
   * throws the old spans away and the ranges over them with them.
   */
  repaint(): void {
    if (this.matches.length === 0) return;
    const current = this.matches[this.at];
    const all: Range[] = [];
    const focused: Range[] = [];
    for (const match of this.matches) {
      const ranges = this.rangesFor(match);
      all.push(...ranges);
      if (match === current) focused.push(...ranges);
    }
    paint(all, focused);
  }

  /** Forget the results — a new document has nothing to do with the old one's. */
  reset(): void {
    this.generation += 1;
    this.matches = [];
    this.at = -1;
    this.total = 0;
    this.describe();
    clearHighlights();
  }

  dispose(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = undefined;
    this.generation += 1;
    clearHighlights();
  }

  private describe(): void {
    this.label = findLabel(this.query, this.total, this.at);
  }

  /** Bring the current match on screen and paint it. */
  private async show(): Promise<void> {
    const match = this.matches[this.at];
    const column = this.context.column();
    if (!match || !column) {
      clearHighlights();
      return;
    }
    // Scroll first so the page enters the render band, then wait for the draw
    // that scroll started rather than starting a second one.
    column.goToPage(match.page);
    await column.ensureDrawn(match.page);
    this.repaint();

    const range = this.rangesFor(match)[0];
    if (range) this.context.reveal(range);
  }

  /** The ranges one match covers, or none if its page is not rendered. */
  private rangesFor(match: Match): Range[] {
    const column = this.context.column();
    if (!column) return [];
    const divs = column.textDivs(match.page);
    const items = column.cachedText(match.page);
    if (!divs || !items) return [];

    const { spans } = joinItems(items);
    const ranges: Range[] = [];
    for (const slice of sliceMatch(spans, match)) {
      const div = divs[slice.index];
      if (!div) continue;
      const range = rangeIn(div, slice.start, slice.end);
      if (range) ranges.push(range);
    }
    return ranges;
  }
}
