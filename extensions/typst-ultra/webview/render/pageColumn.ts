import type { PageFormat, PageMetric, PagePatch } from '../../src/preview/messages.js';
import { PX_PER_PT, clampZoom, type PageGeom, type ViewSize } from '../model/layout.js';
import { adopt, rasterPage } from './sanitize.js';

/** How many pages beyond the viewport to keep rendered. */
export const PREFETCH_MARGIN = 1;

/** What a page looks like to the column. */
interface Page {
  metric: PageMetric;
  element: HTMLElement;
  /** The hash currently rendered into `element`, or null for a placeholder. */
  rendered: string | null;
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
  private readonly onScroll = (): void => this.reportViewport();

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

  /** Rebuild the placeholder layout from a fresh measurement. */
  setMetrics(metrics: PageMetric[]): void {
    const existing = new Map(this.pages.map((page) => [page.metric.hash, page]));

    const next: Page[] = metrics.map((metric) => {
      const reused = existing.get(metric.hash);
      if (reused) {
        reused.metric = metric;
        return reused;
      }
      return { metric, element: this.makePage(metric), rendered: null };
    });

    // Replace children in one pass so the browser lays out once.
    this.container.replaceChildren(...next.map((page) => page.element));
    this.pages = next;
    this.applyZoom();
    this.reportViewport();
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

  /** Scroll so a document point is visible, and return the element to mark. */
  reveal(index: number, yPt: number): HTMLElement | null {
    const page = this.pages[index];
    if (!page) return null;

    const offset = yPt * PX_PER_PT * this.zoom;
    this.container.scrollTop = page.element.offsetTop + offset - this.container.clientHeight / 3;
    return page.element;
  }

  /** The page currently nearest the middle of the viewport. */
  centerPage(): { page: number; yPt: number } | null {
    const middle = this.container.scrollTop + this.container.clientHeight / 2;

    for (const [index, page] of this.pages.entries()) {
      const top = page.element.offsetTop;
      const bottom = top + page.element.offsetHeight;
      if (middle >= top && middle <= bottom) {
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
    const top = this.container.scrollTop;
    for (const [index, page] of this.pages.entries()) {
      const pageTop = page.element.offsetTop;
      const height = page.element.offsetHeight;
      if (top < pageTop + height) {
        return { index, ratio: height > 0 ? (top - pageTop) / height : 0 };
      }
    }
    return null;
  }

  /** Put the reader back on an anchor taken before a rescale. */
  restore(anchor: Anchor): void {
    const page = this.pages[anchor.index];
    if (!page) return;
    this.container.scrollTop = page.element.offsetTop + anchor.ratio * page.element.offsetHeight;
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
    this.container.replaceChildren();
    this.container.scrollTop = 0;
  }

  /** Tell the host what is visible and what we already hold. */
  reportViewport(): void {
    if (this.pages.length === 0) return;

    const top = this.container.scrollTop;
    const bottom = top + this.container.clientHeight;

    let first = this.pages.length - 1;
    let last = 0;
    for (const [index, page] of this.pages.entries()) {
      const pageTop = page.element.offsetTop;
      const pageBottom = pageTop + page.element.offsetHeight;
      if (pageBottom >= top && pageTop <= bottom) {
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
    this.reset();
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

    page.element.querySelector('svg, img.page-raster')?.remove();

    page.element.append(parsed.cloneNode(true));
    page.element.classList.remove('placeholder');
    page.rendered = hash;
  }

  private clear(page: Page): void {
    if (!page.rendered) return;
    page.element.querySelector('svg, img.page-raster')?.remove();
    page.element.classList.add('placeholder');
    page.rendered = null;
  }

  private applyZoom(): void {
    for (const page of this.pages) {
      const width = page.metric.widthPt * PX_PER_PT * this.zoom;
      const height = page.metric.heightPt * PX_PER_PT * this.zoom;
      page.element.style.width = `${width}px`;
      page.element.style.height = `${height}px`;
    }
  }
}
