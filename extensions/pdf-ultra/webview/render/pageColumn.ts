import type { PDFDocumentProxy, RenderTask } from 'pdfjs-dist';
import type { Rotation } from '../../src/messages.js';
import type { PageItem } from '../model/find.js';
import {
  currentPage,
  isWithinBand,
  offsetRatio,
  pointOnPage,
  rasterRatio,
  scrollTopAt,
  scrollTopFor,
  stackPages,
  viewportScale,
  type PageBox,
  type PageGeom,
} from '../model/layout.js';
import { pdfjs } from './pdfjs.js';

/** What a click on a link annotation asks for. */
export type LinkTarget =
  | { kind: 'url'; href: string }
  | { kind: 'dest'; dest: string | unknown[] };

/** How the column is told to behave. Follows the extension's settings. */
export interface ColumnOptions {
  textLayer: boolean;
  links: boolean;
  maxCanvasPixels: number;
  renderAhead: number;
}

export interface ColumnCallbacks {
  /** The page the reader is looking at changed. 1-based. */
  onPage(page: number): void;
  /** A link annotation was clicked. */
  onLink(target: LinkTarget): void;
  /** A page's text layer was (re)built — find highlights have to be reapplied. */
  onTextLayer(page: number): void;
}

/**
 * One page's slot in the scrolling column: a box that always occupies the
 * page's laid-out size, whether or not it currently holds a raster.
 */
interface PageSlot {
  /** 1-based. */
  readonly n: number;
  readonly el: HTMLElement;
  readonly canvas: HTMLCanvasElement;
  readonly text: HTMLElement;
  readonly links: HTMLElement;
  box: PageBox;
  /** Zoom the resident raster was drawn at; 0 when the slot is empty. */
  drawnAt: number;
  /** Zoom of the in-flight draw, if any. */
  drawingAt: number;
  task: RenderTask | null;
  /** The in-flight draw, so a caller that needs the page can await it. */
  pending: Promise<void> | null;
  /** The text runs of the rendered layer, in `textDivs` order. */
  textDivs: HTMLElement[] | null;
  /** Bumped to invalidate in-flight async work for this slot. */
  seq: number;
}

/**
 * The virtualized page column.
 *
 * Scrolling is the navigation: every page has a slot sized from its geometry up
 * front, so the scrollbar tells the truth about the whole document, but only
 * the pages within a screen or so of the visible band hold a raster. The next
 * page rasterizes just before it scrolls into view, and pages that leave
 * release their canvas — a 500-page document costs roughly what a 5-page one
 * costs.
 *
 * Offsets come from the layout model, never from `getBoundingClientRect`: a
 * 500-slot pass costs arithmetic, not a forced reflow.
 */
export class PageColumn {
  private doc: PDFDocumentProxy | null = null;
  private slots: PageSlot[] = [];
  /** Unrotated page geometry, in points, corrected as pages actually render. */
  private geom = new Map<number, PageGeom>();
  /** Page 1's size, standing in for every page not yet measured. */
  private base: PageGeom = { w: 612, h: 792 };
  private zoom = 1;
  private rotation: Rotation = 0;
  private options: ColumnOptions;
  /** Lazily extracted page text, for find. */
  private readonly textCache = new Map<number, PageItem[]>();
  private syncQueued = false;
  private reported = 0;
  /**
   * The scroller size and zoom the current layout was computed for. A resize
   * that changes neither — a scrollbar appearing and disappearing — must not
   * start a relayout loop.
   */
  private laidOutFor = '';
  /** Guards stale async work across document swaps and teardown. */
  private generation = 0;

  private readonly onScroll = (): void => this.queueSync();

  /**
   * Resizing is the element's business, not the column's: a fit is a zoom that
   * depends on the scroller's width, so the element recomputes it and then
   * calls {@link relayout}. A second observer here would relayout at the old
   * zoom first and undo itself a frame later.
   */
  constructor(
    private readonly scroll: HTMLElement,
    private readonly host: HTMLElement,
    private readonly callbacks: ColumnCallbacks,
    options: ColumnOptions,
  ) {
    this.options = options;
    this.scroll.addEventListener('scroll', this.onScroll, { passive: true });
  }

  get pageCount(): number {
    return this.slots.length;
  }

  /** The page the reader is looking at, 1-based. */
  get page(): number {
    const at = currentPage(
      this.slots.map((slot) => slot.box),
      this.scroll.scrollTop,
      this.scroll.clientHeight,
    );
    return at ?? 1;
  }

  /** How far into the current page the viewport starts. */
  get offset(): number {
    const slot = this.slots[this.page - 1];
    return slot ? offsetRatio(slot.box, this.scroll.scrollTop) : 0;
  }

  get scale(): number {
    return this.zoom;
  }

  /** Page 1's unrotated size, which the fits are computed against. */
  get baseGeom(): PageGeom {
    return this.base;
  }

  get viewSize(): { w: number; h: number } {
    return { w: this.scroll.clientWidth, h: this.scroll.clientHeight };
  }

  setOptions(options: ColumnOptions): void {
    const before = this.options;
    this.options = options;
    // The text and link layers are built during a draw, so a change to either
    // only lands when the pages are drawn again.
    if (before.textLayer !== options.textLayer || before.links !== options.links) {
      for (const slot of this.slots) this.release(slot);
    }
    this.syncViewport();
  }

  /**
   * Take a document: one slot per page, sized from page 1's geometry and
   * corrected per page as pages actually render. Only the boxes are built here;
   * rasterizing is the viewport sync's job.
   */
  async open(doc: PDFDocumentProxy): Promise<void> {
    const generation = ++this.generation;
    this.teardown();
    this.doc = doc;

    const first = await doc.getPage(1);
    if (generation !== this.generation) return;
    const view = first.getViewport({ scale: 1 });
    this.base = { w: view.width, h: view.height };
    this.geom.set(1, { ...this.base });

    const fragment = document.createDocumentFragment();
    const slots: PageSlot[] = [];
    for (let n = 1; n <= doc.numPages; n += 1) {
      const el = document.createElement('div');
      el.className = 'page placeholder';
      el.dataset.page = String(n);
      el.setAttribute('aria-label', `Page ${n}`);

      const canvas = document.createElement('canvas');
      canvas.className = 'page-canvas';
      canvas.setAttribute('aria-hidden', 'true');

      const text = document.createElement('div');
      text.className = 'page-text';

      const links = document.createElement('div');
      links.className = 'page-links';

      const number = document.createElement('div');
      number.className = 'page-number';
      number.textContent = String(n);

      el.append(canvas, text, links, number);
      fragment.append(el);
      slots.push({
        n,
        el,
        canvas,
        text,
        links,
        box: { w: 0, h: 0, top: 0 },
        drawnAt: 0,
        drawingAt: 0,
        task: null,
        pending: null,
        textDivs: null,
        seq: 0,
      });
    }
    this.host.replaceChildren(fragment);
    this.slots = slots;
    this.laidOutFor = '';
    this.relayout();
  }

  /** Set the zoom and rotation, keeping the reader where they were. */
  setView(zoom: number, rotation: Rotation): void {
    if (zoom === this.zoom && rotation === this.rotation) return;
    this.zoom = zoom;
    this.rotation = rotation;
    this.relayout();
  }

  /**
   * Recompute every slot's box and keep the reader in place — the same spot on
   * the same page, not the top of the document.
   */
  relayout(): void {
    if (this.slots.length === 0) return;
    const view = this.viewSize;
    const signature = `${view.w}x${view.h}@${this.zoom}r${this.rotation}`;
    if (signature === this.laidOutFor) return;
    this.laidOutFor = signature;

    const anchorIndex = Math.min(this.page, this.slots.length) - 1;
    const anchor = this.slots[anchorIndex]!;
    const within = offsetRatio(anchor.box, this.scroll.scrollTop);

    const boxes = stackPages(
      this.slots.map((slot) => this.geom.get(slot.n) ?? this.base),
      viewportScale(this.zoom),
      this.rotation,
    );
    this.slots.forEach((slot, index) => {
      const box = boxes[index]!;
      slot.box = box;
      slot.el.style.width = `${box.w}px`;
      slot.el.style.height = `${box.h}px`;
      // Every resident raster is now the wrong size for its box.
      if (slot.drawnAt !== this.zoom) this.release(slot);
    });
    this.scroll.scrollTop = scrollTopAt(this.slots[anchorIndex]!.box, within);
    this.syncViewport();
  }

  /** Scroll a page to the top of the viewport. 1-based. */
  goToPage(page: number): void {
    const slot = this.slots[clamp(page, 1, this.slots.length) - 1];
    if (!slot) return;
    this.scroll.scrollTop = scrollTopFor(slot.box);
    this.syncViewport();
  }

  /**
   * Scroll to a fraction of the way into a page — the inverse of {@link offset},
   * and how a reopened or reloaded document puts the reader back exactly where
   * they were rather than at the top of the page they were on.
   */
  revealRatio(page: number, ratio: number): void {
    const slot = this.slots[clamp(page, 1, this.slots.length) - 1];
    if (!slot) return;
    this.scroll.scrollTop = scrollTopAt(slot.box, ratio);
    this.syncViewport();
  }

  /**
   * Scroll so a point in PDF user space is a third of the way down the
   * viewport — where a jumped-to destination reads as arrived at rather than
   * as clipped to the top edge.
   */
  revealPoint(page: number, point: { x: number; y: number } | null): void {
    const slot = this.slots[clamp(page, 1, this.slots.length) - 1];
    if (!slot) return;
    if (!point) {
      this.goToPage(page);
      return;
    }
    const base = this.geom.get(slot.n) ?? this.base;
    const on = pointOnPage(base, this.rotation, point, viewportScale(this.zoom));
    this.scroll.scrollTop = Math.max(
      0,
      slot.box.top + on.y - this.scroll.clientHeight / 3,
    );
    this.syncViewport();
  }

  /** Ensure a page holds a raster before something is measured against it. */
  async ensureDrawn(page: number): Promise<void> {
    const slot = this.slots[page - 1];
    if (!slot) return;
    await this.draw(slot);
  }

  /** The text runs of a rendered page, in `textDivs` order, or null. */
  textDivs(page: number): HTMLElement[] | null {
    return this.slots[page - 1]?.textDivs ?? null;
  }

  /** A page's text if it has already been extracted; never a fetch. */
  cachedText(page: number): PageItem[] | undefined {
    return this.textCache.get(page);
  }

  /**
   * A page's text, extracted once and kept.
   *
   * Independent of whether the page is rendered: find has to see the whole
   * document, and only a screen of it is ever drawn.
   */
  async textItems(page: number): Promise<PageItem[]> {
    const cached = this.textCache.get(page);
    if (cached) return cached;
    const doc = this.doc;
    if (!doc) return [];
    const generation = this.generation;
    try {
      const proxy = await doc.getPage(page);
      const content = await proxy.getTextContent();
      const items: PageItem[] = content.items.map((item) =>
        'str' in item
          ? { text: item.str, eol: item.hasEOL === true }
          : { text: '', eol: false },
      );
      if (generation === this.generation) this.textCache.set(page, items);
      return items;
    } catch {
      return [];
    }
  }

  /**
   * Render one page to a PNG, off to the side of the column.
   *
   * Deliberately not the on-screen canvas: that one is sized for the display
   * and may be at a coarser ratio than the cap allows, and an export should be
   * the page at a print-ish resolution rather than a screenshot of the viewer.
   */
  async renderPng(page: number, zoom: number): Promise<string | null> {
    const doc = this.doc;
    if (!doc) return null;
    const proxy = await doc.getPage(page);
    const viewport = proxy.getViewport({
      scale: viewportScale(zoom),
      rotation: (proxy.rotate + this.rotation) % 360,
    });
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(viewport.width));
    canvas.height = Math.max(1, Math.round(viewport.height));
    await proxy.render({ canvas, viewport }).promise;
    const url = canvas.toDataURL('image/png');
    canvas.width = 0;
    canvas.height = 0;
    return url.slice(url.indexOf(',') + 1);
  }

  /** Coalesce scroll bursts into one pass per frame. */
  private queueSync(): void {
    if (this.syncQueued) return;
    this.syncQueued = true;
    const run = (): void => {
      this.syncQueued = false;
      this.syncViewport();
    };
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(run);
    else queueMicrotask(run);
  }

  /**
   * Draw what is on screen plus a margin either way — which is what makes
   * scrolling continuous rather than a series of blank frames — release what is
   * not, and report the page the reader is actually looking at.
   */
  private syncViewport(): void {
    if (this.slots.length === 0) return;
    const top = this.scroll.scrollTop;
    const view = this.scroll.clientHeight;
    for (const slot of this.slots) {
      if (isWithinBand(slot.box, top, view, this.options.renderAhead)) {
        void this.draw(slot);
      } else if (slot.drawnAt !== 0 || slot.drawingAt !== 0) {
        this.release(slot);
      }
    }
    const at = currentPage(
      this.slots.map((slot) => slot.box),
      top,
      view,
    );
    if (at !== null && at !== this.reported) {
      this.reported = at;
      this.callbacks.onPage(at);
    }
  }

  /**
   * Rasterize one page into its slot and rebuild its text and link layers.
   *
   * Idempotent per zoom, so the sync pass can call it for every visible slot on
   * every frame — and a caller that needs the page *now* (find, export) gets
   * the draw already in flight rather than a second one.
   */
  private draw(slot: PageSlot): Promise<void> {
    const zoom = this.zoom;
    if (slot.drawnAt === zoom) return Promise.resolve();
    if (slot.drawingAt === zoom && slot.pending) return slot.pending;
    slot.pending = this.drawNow(slot, zoom);
    return slot.pending;
  }

  private async drawNow(slot: PageSlot, zoom: number): Promise<void> {
    const doc = this.doc;
    if (!doc) return;
    slot.drawingAt = zoom;
    const seq = ++slot.seq;
    const generation = this.generation;
    const stale = (): boolean => generation !== this.generation || seq !== slot.seq;

    try {
      const proxy = await doc.getPage(slot.n);
      if (stale()) return;
      const unrotated = proxy.getViewport({ scale: 1, rotation: proxy.rotate });
      this.noteGeometry(slot, unrotated.width, unrotated.height);

      const viewport = proxy.getViewport({
        scale: viewportScale(zoom),
        rotation: (proxy.rotate + this.rotation) % 360,
      });
      const ratio = rasterRatio(
        slot.box,
        window.devicePixelRatio || 1,
        this.options.maxCanvasPixels,
      );
      // Only the bitmap is sized here — the canvas and the layers over it are
      // stretched across the slot by CSS, so nothing they carry can outlive the
      // raster and stretch the scroll extent.
      slot.canvas.width = Math.max(1, Math.round(viewport.width * ratio));
      slot.canvas.height = Math.max(1, Math.round(viewport.height * ratio));
      slot.task = proxy.render({
        canvas: slot.canvas,
        viewport,
        transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0],
      });
      await slot.task.promise;
      slot.task = null;
      if (stale()) return;
      slot.el.classList.remove('placeholder');

      if (this.options.textLayer) {
        // The text layer is what makes the page selectable, findable and
        // readable by a screen reader; without it a page is a picture of words.
        slot.text.replaceChildren();
        slot.text.style.setProperty('--scale-factor', String(viewportScale(zoom)));
        const layer = new pdfjs.TextLayer({
          textContentSource: proxy.streamTextContent(),
          container: slot.text,
          viewport,
        });
        await layer.render();
        if (stale()) return;
        slot.textDivs = layer.textDivs;
        this.callbacks.onTextLayer(slot.n);
      }

      if (this.options.links) await this.drawLinks(slot, proxy, stale);

      slot.drawnAt = zoom;
      slot.drawingAt = 0;
    } catch {
      // A cancelled render (scrolled away, rezoomed) or a page that will not
      // rasterize is not a dead document — the slot stays blank at the right
      // size and the next pass over it retries.
      if (slot.drawingAt === zoom) slot.drawingAt = 0;
      slot.task = null;
    } finally {
      if (slot.pending !== null && slot.drawingAt !== zoom) slot.pending = null;
    }
  }

  /**
   * Rebuild a page's link annotations as plain anchors.
   *
   * pdf.js's own annotation layer is not mounted: it renders form widgets and
   * wires up a scripting surface, neither of which a read-only viewer wants
   * anywhere near an untrusted document. Only `Link` subtypes are read, and a
   * click on one is a message to the host — the page itself never navigates.
   */
  private async drawLinks(
    slot: PageSlot,
    proxy: Awaited<ReturnType<PDFDocumentProxy['getPage']>>,
    stale: () => boolean,
  ): Promise<void> {
    const annotations = await proxy.getAnnotations({ intent: 'display' });
    if (stale()) return;
    slot.links.replaceChildren();

    const base = this.geom.get(slot.n) ?? this.base;
    const scale = viewportScale(this.zoom);
    for (const annotation of annotations) {
      if (annotation.subtype !== 'Link') continue;
      const target: LinkTarget | null =
        typeof annotation.url === 'string'
          ? { kind: 'url', href: annotation.url }
          : annotation.dest
            ? { kind: 'dest', dest: annotation.dest as string | unknown[] }
            : null;
      if (!target) continue;

      const [x1, y1, x2, y2] = annotation.rect as [number, number, number, number];
      const a = pointOnPage(base, this.rotation, { x: x1, y: y1 }, scale);
      const b = pointOnPage(base, this.rotation, { x: x2, y: y2 }, scale);

      const element = document.createElement('a');
      element.className = 'page-link';
      element.style.left = `${Math.min(a.x, b.x)}px`;
      element.style.top = `${Math.min(a.y, b.y)}px`;
      element.style.width = `${Math.abs(b.x - a.x)}px`;
      element.style.height = `${Math.abs(b.y - a.y)}px`;
      element.title = target.kind === 'url' ? target.href : 'Go to destination';
      // No `href`: a link that looks navigable in a webview is a link that can
      // take the frame somewhere. The click handler is the only way out.
      element.setAttribute('role', 'link');
      element.tabIndex = 0;
      const go = (event: Event): void => {
        event.preventDefault();
        this.callbacks.onLink(target);
      };
      element.addEventListener('click', go);
      element.addEventListener('keydown', (event) => {
        if (event.key === 'Enter' || event.key === ' ') go(event);
      });
      slot.links.append(element);
    }
  }

  /** Drop a slot's raster and layers, keeping its box. */
  private release(slot: PageSlot): void {
    slot.seq += 1;
    slot.task?.cancel();
    slot.task = null;
    slot.pending = null;
    slot.drawnAt = 0;
    slot.drawingAt = 0;
    slot.canvas.width = 0;
    slot.canvas.height = 0;
    slot.text.replaceChildren();
    slot.links.replaceChildren();
    slot.textDivs = null;
    slot.el.classList.add('placeholder');
  }

  /**
   * A page turned out not to be page 1's size. Correct its box and slide the
   * pages below it, compensating the scroll position when the correction lands
   * above the viewport so the reader does not jump.
   */
  private noteGeometry(slot: PageSlot, w: number, h: number): void {
    const previous = this.geom.get(slot.n);
    if (previous && previous.w === w && previous.h === h) return;
    this.geom.set(slot.n, { w, h });

    const scale = viewportScale(this.zoom);
    const display =
      this.rotation === 90 || this.rotation === 270 ? { w: h, h: w } : { w, h };
    const nextW = Math.max(1, Math.round(display.w * scale));
    const nextH = Math.max(1, Math.round(display.h * scale));
    if (nextW === slot.box.w && nextH === slot.box.h) return;

    const delta = nextH - slot.box.h;
    slot.box = { ...slot.box, w: nextW, h: nextH };
    slot.el.style.width = `${nextW}px`;
    slot.el.style.height = `${nextH}px`;
    if (delta === 0) return;
    for (const other of this.slots) {
      if (other.n > slot.n) other.box = { ...other.box, top: other.box.top + delta };
    }
    if (slot.box.top + slot.box.h <= this.scroll.scrollTop) {
      this.scroll.scrollTop += delta;
    }
  }

  private teardown(): void {
    this.generation += 1;
    for (const slot of this.slots) this.release(slot);
    this.slots = [];
    this.geom.clear();
    this.textCache.clear();
    this.reported = 0;
    this.laidOutFor = '';
    this.host.replaceChildren();
    this.scroll.scrollTop = 0;
  }

  destroy(): void {
    this.teardown();
    this.scroll.removeEventListener('scroll', this.onScroll);
    this.doc = null;
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}
