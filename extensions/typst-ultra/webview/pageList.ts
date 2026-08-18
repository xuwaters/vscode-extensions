import type { PageFormat, PageMetric, PagePatch } from '../src/preview/messages.js';

/** How many pages beyond the viewport to keep rendered. */
export const PREFETCH_MARGIN = 1;

/** One point is 1/72 inch; CSS pixels are 1/96, so a point is 4/3 of a pixel. */
export const PX_PER_PT = 96 / 72;

/** What a page looks like to the list. */
interface Page {
  metric: PageMetric;
  element: HTMLElement;
  /** The hash currently rendered into `element`, or null for a placeholder. */
  rendered: string | null;
}

/**
 * The virtualized page list.
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
 */
export class PageList {
  private pages: Page[] = [];
  private zoom = 1;
  /** Rendered pages kept by hash, so a shifted page is reused rather than refetched. */
  private readonly byHash = new Map<string, Element>();

  constructor(
    private readonly container: HTMLElement,
    private readonly onViewportChanged: (
      first: number,
      last: number,
      known: Record<number, string>,
    ) => void,
    private readonly onPageClicked: (page: number, xPt: number, yPt: number) => void,
  ) {
    this.container.addEventListener('scroll', () => this.reportViewport(), {
      passive: true,
    });
    window.addEventListener('resize', () => this.reportViewport(), { passive: true });
  }

  /** How many pages the document has. */
  get length(): number {
    return this.pages.length;
  }

  /** The current zoom factor. */
  get scale(): number {
    return this.zoom;
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

  /** Set the zoom factor. */
  setZoom(zoom: number): void {
    this.zoom = Math.min(20, Math.max(0.1, zoom));
    this.applyZoom();
    this.reportViewport();
  }

  /** Fit the page width, or the whole page, to the container. */
  fit(mode: 'width' | 'page' | 'actual'): number {
    const first = this.pages[0]?.metric;
    if (!first) return this.zoom;

    if (mode === 'actual') {
      this.setZoom(1);
      return this.zoom;
    }

    const availableWidth = this.container.clientWidth - 48;
    const availableHeight = this.container.clientHeight - 48;
    const widthScale = availableWidth / (first.widthPt * PX_PER_PT);
    const heightScale = availableHeight / (first.heightPt * PX_PER_PT);

    this.setZoom(mode === 'width' ? widthScale : Math.min(widthScale, heightScale));
    return this.zoom;
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
   * Drop the whole document, back to an empty list at the top.
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

    this.onViewportChanged(first, last, known);
  }

  private makePage(metric: PageMetric): HTMLElement {
    const element = document.createElement('div');
    element.className = 'page';
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

    this.onPageClicked(index, xPt, yPt);
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

/**
 * Parse page SVG and strip anything that should not be there.
 *
 * The compiler emits a fixed vocabulary and cannot be made to emit `<script>`,
 * so this should always be a no-op. It is here because "should always" is not
 * "does", and surviving a compiler bug is better than executing it. The CSP
 * already makes an injected script unrunnable; this is the second layer.
 *
 * Parsing rather than concatenating also means a malformed document yields
 * nothing rather than a half-built DOM.
 */
export function adopt(svg: string): Element | null {
  const parsed = new DOMParser().parseFromString(svg, 'image/svg+xml');
  if (parsed.getElementsByTagName('parsererror').length > 0) return null;

  // Two bits of tolerance, both for the same reason: DOM implementations differ
  // on how an XML document exposes its root. Browsers populate
  // `documentElement`; some others only populate `firstElementChild`. And the
  // check is by tag name rather than `instanceof SVGElement`, because which
  // constructor the root is built from also varies — what matters is that it
  // says `<svg>`.
  const root = parsed.documentElement ?? parsed.firstElementChild;
  if (!root || root.tagName.toLowerCase() !== 'svg') return null;

  for (const element of Array.from(root.querySelectorAll('script, foreignObject'))) {
    element.remove();
  }
  for (const element of Array.from(root.querySelectorAll('*'))) {
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      if (name.startsWith('on') || (name === 'href' && isScriptUrl(attribute.value))) {
        element.removeAttribute(attribute.name);
      }
    }
  }

  // The page element already carries the size; let the SVG fill it.
  root.setAttribute('width', '100%');
  root.setAttribute('height', '100%');
  return root;
}

function isScriptUrl(value: string): boolean {
  return /^\s*(javascript|data:text\/html|vbscript)/i.test(value);
}

/**
 * Wrap a base64 PNG as an `<img>`.
 *
 * The `data:` URI is what the CSP's `img-src` permits, and a raster page cannot
 * execute anything — which is why PNG mode needs none of the stripping SVG does.
 * The trade is what decision 0006 names: no zoom fidelity beyond the rendered
 * resolution, and no find-in-preview, because there is no text.
 */
export function rasterPage(base64: string): Element | null {
  if (!/^[A-Za-z0-9+/]+={0,2}$/.test(base64)) return null;

  const image = document.createElement('img');
  image.className = 'page-raster';
  image.src = `data:image/png;base64,${base64}`;
  image.decoding = 'async';
  return image;
}
