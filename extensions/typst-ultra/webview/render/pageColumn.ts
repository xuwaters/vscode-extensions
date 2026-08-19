import type { PageFormat, PageMetric, PagePatch } from '../../src/preview/messages.js';
import { PX_PER_PT, clampZoom, type PageGeom, type ViewSize } from '../model/layout.js';
import { adopt, rasterPage } from './sanitize.js';

/** How many pages beyond the viewport to keep rendered. */
export const PREFETCH_MARGIN = 1;

/** The strip at each edge of the viewport that `reveal` does not count as visible. */
export const REVEAL_MARGIN = 0.1;

/** What a page looks like to the column. */
interface Page {
  metric: PageMetric;
  element: HTMLElement;
  /** The hash currently rendered into `element`, or null for a placeholder. */
  rendered: string | null;
}

/** Where every page starts and how tall it is, in scroller coordinates. */
interface Geometry {
  tops: number[];
  heights: number[];
}

/** Where the reader stands, in terms a change of zoom does not move. */
export interface Anchor {
  index: number;
  /** How far into that page the top of the viewport sits, as a fraction. */
  ratio: number;
}

/** What the column tells the element about. */
export interface ColumnCallbacks {
  onViewport(first: number, last: number, known: Record<number, string>): void;
  onClick(page: number, xPt: number, yPt: number): void;
}

/**
 * The virtualized page column.
 *
 * A text-heavy A4 page is ~386 KB of SVG and a 30-page document is ~11.4 MB, so
 * the webview renders a *window*, not a document. Placeholders sized from the
 * server's `PageMetrics` keep the scrollbar correct for the whole document from
 * the first frame; only pages in the viewport plus one page of margin carry
 * real SVG.
 *
 * Page identity is the content hash, not the index. Insert a paragraph on page
 * 2 of a 60-page document and pages 3–60 shift by one but keep their hashes, so
 * the already-rendered SVG is re-anchored rather than re-fetched.
 *
 * Imperative on purpose, and the reason the element does not own this: a
 * binding per page would be a binding per SVG, and the whole point of the
 * column is that most pages have none. Everything the *reader can see the state
 * of* — the zoom, the fit, the page number — lives in the element instead.
 */
export class PageColumn {
  private pages: Page[] = [];
  private zoom = 1;
  /** Rendered pages kept by hash, so a shifted page is reused rather than refetched. */
  private readonly byHash = new Map<string, Element>();
  /** Page tops and heights, measured once — see {@link measure}. */
  private geometry: Geometry | null = null;
  /** A viewport report waiting for the next frame, or 0. */
  private frame = 0;
  private readonly onScroll = (): void => this.scheduleReport();

  constructor(
    private readonly container: HTMLElement,
    private readonly callbacks: ColumnCallbacks,
  ) {
    this.container.addEventListener('scroll', this.onScroll, { passive: true });
  }

  /** How many pages the document has. */
  get length(): number {
    return this.pages.length;
  }

  /** The current zoom factor. */
  get scale(): number {
    return this.zoom;
  }

  /** The scroller's content box — what a fit is measured against. */
  get viewSize(): ViewSize {
    return { w: this.container.clientWidth, h: this.container.clientHeight };
  }

  /**
   * The page a fit is measured against.
   *
   * The first one. A typst document can mix page sizes, and fitting each page
   * to its own width would rescale the document as the reader scrolls — so the
   * document has one scale and the first page is the one that sets it.
   */
  get baseGeom(): PageGeom | null {
    const first = this.pages[0]?.metric;
    return first ? { widthPt: first.widthPt, heightPt: first.heightPt } : null;
  }

  /**
   * Rebuild the placeholder layout from a fresh measurement.
   *
   * Two passes, and the second one is what keeps the preview still while the
   * reader types. A page whose *hash* matches is the same page that merely
   * moved, and keeps its element and its SVG. A page whose hash matches nothing
   * is usually the page being edited — its content changed a moment ago and its
   * replacement is one round trip away — so it inherits the element that was at
   * its index, stale SVG and all, rather than being built as a fresh
   * placeholder. `rendered` deliberately keeps the *old* hash: the viewport
   * report then tells the server what we are actually showing, the server
   * answers `replace` because the hashes differ, and the page is swapped in one
   * move. Building a placeholder instead would blank the page the reader is
   * looking at on every keystroke and fill it back in a round trip later, which
   * is exactly the flicker.
   */
  setMetrics(metrics: PageMetric[]): void {
    const byHash = new Map<string, Page>();
    for (const page of this.pages) {
      if (!byHash.has(page.metric.hash)) byHash.set(page.metric.hash, page);
    }
    const claimed = new Set<Page>();

    const next: (Page | undefined)[] = metrics.map((metric) => {
      const reused = byHash.get(metric.hash);
      if (!reused || claimed.has(reused)) return undefined;
      claimed.add(reused);
      return this.adoptMetric(reused, metric);
    });

    for (const [index, metric] of metrics.entries()) {
      if (next[index]) continue;
      const stale = this.pages[index];
      if (stale && !claimed.has(stale)) {
        claimed.add(stale);
        next[index] = this.adoptMetric(stale, metric);
      } else {
        next[index] = { metric, element: this.makePage(metric), rendered: null };
      }
    }

    const pages = next as Page[];
    // Only touch the DOM when the child list really differs. A keystroke that
    // reflows nothing leaves every element in place, and detaching and
    // reattaching them all would drop the browser's rasterization of every
    // visible page for nothing — and leave the measured layout to take again.
    if (
      pages.length !== this.pages.length ||
      pages.some((page, index) => page !== this.pages[index])
    ) {
      // Replace children in one pass so the browser lays out once.
      this.container.replaceChildren(...pages.map((page) => page.element));
      this.geometry = null;
    }
    this.pages = pages;
    this.applyZoom();
    this.reportViewport();
  }

  /** Move a surviving page onto its new metric, number and all. */
  private adoptMetric(page: Page, metric: PageMetric): Page {
    if (page.metric.index !== metric.index) {
      page.element.dataset.index = String(metric.index);
      const number = page.element.querySelector('.page-number');
      if (number) number.textContent = String(metric.index + 1);
    }
    page.metric = metric;
    return page;
  }

  /** Apply page patches from the server. */
  applyPatches(patches: PagePatch[]): void {
    for (const patch of patches) {
      const page = this.pages[patch.index];
      if (!page) continue;

      if (patch.op === 'removed') {
        this.clear(page);
      } else if (patch.op === 'replace') {
        this.render(page, patch.hash, patch.format, patch.content);
      }
      // `unchanged` means exactly that: the client's copy is current.
    }
  }

  /**
   * Set the zoom factor, keeping the reader where they were.
   *
   * The anchor is what makes a live fit bearable: dragging the split wider
   * re-resolves `fit-width` on every frame, and without holding the page still
   * the document would walk away under the pointer.
   */
  setZoom(zoom: number): void {
    const next = clampZoom(zoom);
    if (next === this.zoom) return;
    const anchor = this.anchor();
    this.zoom = next;
    this.applyZoom();
    if (anchor) this.restore(anchor);
    this.reportViewport();
  }

  /** Scroll a page into view. */
  goToPage(index: number): void {
    const page = this.pages[index];
    if (page) page.element.scrollIntoView({ block: 'start', behavior: 'auto' });
  }

  /**
   * Scroll so a document point is visible, and return the element to mark.
   *
   * Only when it is not already — the same rule `revealRange` follows with
   * `InCenterIfOutsideViewport`, and here it is what keeps the preview still
   * while the reader types. Every keystroke moves the cursor, and scrolling to
   * a point already on screen would twitch the page on each one.
   *
   * The band excludes a strip at each edge, so a cursor sitting on the last
   * visible line is brought properly into view rather than left half off it.
   */
  reveal(index: number, yPt: number): HTMLElement | null {
    const page = this.pages[index];
    if (!page) return null;

    const target = this.measure().tops[index] + yPt * PX_PER_PT * this.zoom;
    const view = this.container.clientHeight;
    const top = this.container.scrollTop;
    const margin = view * REVEAL_MARGIN;

    if (target >= top + margin && target <= top + view - margin) return page.element;

    this.container.scrollTop = target - view / 3;
    return page.element;
  }

  /** The page currently nearest the middle of the viewport. */
  centerPage(): { page: number; yPt: number } | null {
    const { tops, heights } = this.measure();
    const middle = this.container.scrollTop + this.container.clientHeight / 2;

    for (let index = 0; index < this.pages.length; index += 1) {
      const top = tops[index];
      if (middle >= top && middle <= top + heights[index]) {
        return { page: index, yPt: (middle - top) / (PX_PER_PT * this.zoom) };
      }
    }
    return null;
  }

  /**
   * Where the reader stands, as a page and a fraction of it.
   *
   * Scale-free by construction — every page's height is linear in the zoom — so
   * the same anchor means the same place before and after a rescale.
   */
  anchor(): Anchor | null {
    const { tops, heights } = this.measure();
    const top = this.container.scrollTop;
    for (let index = 0; index < this.pages.length; index += 1) {
      const pageTop = tops[index];
      const height = heights[index];
      if (top < pageTop + height) {
        return { index, ratio: height > 0 ? (top - pageTop) / height : 0 };
      }
    }
    return null;
  }

  /** Put the reader back on an anchor taken before a rescale. */
  restore(anchor: Anchor): void {
    if (!this.pages[anchor.index]) return;
    const { tops, heights } = this.measure();
    this.container.scrollTop =
      tops[anchor.index] + anchor.ratio * heights[anchor.index];
  }

  /**
   * Drop everything rendered, so the next viewport report asks for it again.
   *
   * Needed when the *format* a page should arrive in changes — a raster mode
   * switch, or a zoom step in raster mode — because the page hash identifies the
   * page's content, not its rendering.
   */
  forget(): void {
    this.byHash.clear();
    for (const page of this.pages) this.clear(page);
    this.reportViewport();
  }

  /**
   * Drop the whole document, back to an empty column at the top.
   *
   * The subject changed — the panel follows the active editor, and the reader
   * clicked a different `.typ`. Keeping the old pages would leave the previous
   * document on screen until the first patch of the new one lands, and keeping
   * the scroll position would open a two-page letter at page 40 of the book it
   * replaced.
   */
  reset(): void {
    this.byHash.clear();
    this.pages = [];
    this.geometry = null;
    this.container.replaceChildren();
    this.container.scrollTop = 0;
  }

  /** Tell the host what is visible and what we already hold. */
  reportViewport(): void {
    if (this.pages.length === 0) return;

    const { tops, heights } = this.measure();
    const top = this.container.scrollTop;
    const bottom = top + this.container.clientHeight;

    let first = this.pages.length - 1;
    let last = 0;
    for (let index = 0; index < this.pages.length; index += 1) {
      if (tops[index] + heights[index] >= top && tops[index] <= bottom) {
        first = Math.min(first, index);
        last = Math.max(last, index);
      }
    }

    first = Math.max(0, first - PREFETCH_MARGIN);
    last = Math.min(this.pages.length - 1, last + PREFETCH_MARGIN);

    // Anything outside the window plus its margin goes back to a placeholder,
    // which is what bounds DOM size on a 200-page document.
    for (const [index, page] of this.pages.entries()) {
      if (index < first || index > last) this.clear(page);
    }

    const known: Record<number, string> = {};
    for (let index = first; index <= last; index += 1) {
      const rendered = this.pages[index]?.rendered;
      if (rendered) known[index] = rendered;
    }

    this.callbacks.onViewport(first, last, known);
  }

  /** The element the cursor indicator is hung on, for a given page. */
  pageElement(index: number): HTMLElement | null {
    return this.pages[index]?.element ?? null;
  }

  destroy(): void {
    this.container.removeEventListener('scroll', this.onScroll);
    if (this.frame !== 0 && typeof cancelAnimationFrame !== 'undefined') {
      cancelAnimationFrame(this.frame);
    }
    this.frame = 0;
    this.reset();
  }

  /**
   * Report the viewport at most once per frame.
   *
   * A scroll fires far more often than the screen is painted, and the report is
   * not a read: it takes every page outside the window back down to a
   * placeholder. Doing that several times between two frames is work no one can
   * see. Only the *scroll* goes through here — everything else that changes the
   * window reports straight away, because it has a round trip waiting on it.
   */
  private scheduleReport(): void {
    if (typeof requestAnimationFrame === 'undefined') {
      this.reportViewport();
      return;
    }
    if (this.frame !== 0) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.reportViewport();
    });
  }

  /**
   * Where every page sits, measured once and kept until something moves it.
   *
   * Reading `offsetTop` makes the browser flush any layout it has pending, and
   * a scroll asks three separate questions of the column's geometry — the
   * visible band, the page in the middle, the anchor — at up to a frame apiece.
   * None of that moves a page: the boxes are sized in pixels from the metrics
   * and the zoom, so their offsets change only when the column is rebuilt or
   * rescaled. Both of those drop this, and nothing else has to.
   *
   * Everything is put in the scroller's own space on the way in. `offsetTop` is
   * measured from whichever ancestor happens to be positioned, so it carries
   * the toolbar's height and the column's padding with it; `scrollTop` counts
   * from the top of the column's padding box. Subtracting the column's own
   * offset is what makes the two comparable.
   */
  private measure(): Geometry {
    if (this.geometry) return this.geometry;

    const origin = this.container.offsetTop + this.container.clientTop;
    const tops: number[] = [];
    const heights: number[] = [];
    for (const page of this.pages) {
      tops.push(page.element.offsetTop - origin);
      heights.push(page.element.offsetHeight);
    }
    this.geometry = { tops, heights };
    return this.geometry;
  }

  private makePage(metric: PageMetric): HTMLElement {
    const element = document.createElement('div');
    element.className = 'page placeholder';
    element.dataset.index = String(metric.index);

    const number = document.createElement('div');
    number.className = 'page-number';
    number.textContent = String(metric.index + 1);
    element.append(number);

    element.addEventListener('click', (event) => this.onClick(event, element));
    return element;
  }

  private onClick(event: MouseEvent, element: HTMLElement): void {
    const index = Number(element.dataset.index);
    if (!Number.isInteger(index)) return;

    const bounds = element.getBoundingClientRect();
    // Divide out the zoom here so the server only ever sees document space and
    // never has to know about zoom or device pixel ratio.
    const xPt = (event.clientX - bounds.left) / (PX_PER_PT * this.zoom);
    const yPt = (event.clientY - bounds.top) / (PX_PER_PT * this.zoom);

    this.callbacks.onClick(index, xPt, yPt);
  }

  private render(page: Page, hash: string, format: PageFormat, content: string): void {
    if (page.rendered === hash) return;

    const parsed =
      this.byHash.get(hash) ?? (format === 'png' ? rasterPage(content) : adopt(content));
    if (!parsed) return;

    this.byHash.set(hash, parsed);
    // Bound the cache: the window is small, and holding every page ever seen
    // would defeat virtualization.
    if (this.byHash.size > 64) {
      const oldest = this.byHash.keys().next().value;
      if (oldest !== undefined && oldest !== hash) this.byHash.delete(oldest);
    }

    const fresh = parsed.cloneNode(true) as Element;
    // Claimed before the swap, not after: a raster page waits for its decode
    // below, and the viewport report in between has to name the page that is on
    // its way rather than ask for it a second time.
    page.rendered = hash;

    const swap = (): void => {
      // Superseded while we waited — a newer compile, or the page scrolled out
      // of the window and was cleared.
      if (page.rendered !== hash) return;
      const current = page.element.querySelector('svg, img.page-raster');
      // One mutation, so the old page is only detached at the moment the new
      // one lands. Removing first leaves a frame with nothing in the box.
      if (current) current.replaceWith(fresh);
      else page.element.append(fresh);
      page.element.classList.remove('placeholder');
    };

    // An `<img>` is not drawable the instant it is attached, so swapping one in
    // undecoded shows white where the page was. Vector pages have no such wait.
    if (fresh instanceof HTMLImageElement && !fresh.complete) {
      void fresh.decode().then(swap, swap);
    } else {
      swap();
    }
  }

  private clear(page: Page): void {
    if (!page.rendered) return;
    page.element.querySelector('svg, img.page-raster')?.remove();
    page.element.classList.add('placeholder');
    page.rendered = null;
  }

  /**
   * Size every page box for the current zoom.
   *
   * Guarded rather than written blind: this runs on every compile, and writing
   * the same length back invalidates the style of every page in the column for
   * a layout that cannot have changed — and would throw away the measurement
   * {@link measure} holds along with it.
   */
  private applyZoom(): void {
    let moved = false;
    for (const page of this.pages) {
      const width = `${page.metric.widthPt * PX_PER_PT * this.zoom}px`;
      const height = `${page.metric.heightPt * PX_PER_PT * this.zoom}px`;
      if (page.element.style.width !== width) {
        page.element.style.width = width;
        moved = true;
      }
      if (page.element.style.height !== height) {
        page.element.style.height = height;
        moved = true;
      }
    }
    if (moved) this.geometry = null;
  }
}
