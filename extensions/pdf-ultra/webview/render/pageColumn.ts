import type { PDFDocumentProxy, RenderTask } from 'pdfjs-dist';
import type { PageMode, Rotation } from '../../src/messages.js';
import type { PageItem } from '../model/find.js';
import {
  anchorAt,
  columnsFor,
  currentPage,
  isWithinBand,
  offsetRatio,
  pointOnPage,
  rasterRatio,
  scrollTopAt,
  scrollTopFor,
  stackPages,
  stackSingle,
  viewportScale,
  type PageBox,
  type PageGeom,
} from '../model/layout.js';
import { pdfjs } from './pdfjs.js';

/**
 * How long the column waits for a resize or a zoom to stop moving before it
 * rasterizes again.
 *
 * A drag-resize delivers a new size every animation frame, and a render is far
 * slower than that — so redrawing per frame is a render started and cancelled
 * sixty times a second, none of which ever reaches the screen. The pages are
 * stretched to the new boxes immediately either way; this is only how long they
 * stay soft before the sharp ones land.
 */
const SETTLE_MS = 120;

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
  /**
   * The raster surface. Swapped rather than resized: a draw paints into a
   * canvas of its own and takes this one's place only once it is finished, so
   * the page the reader is looking at is never blanked to make room for it.
   */
  canvas: HTMLCanvasElement;
  readonly text: HTMLElement;
  readonly links: HTMLElement;
  box: PageBox;
  /**
   * The view the resident raster was drawn for — see {@link PageColumn.viewKey}.
   * Empty when the slot holds no raster.
   *
   * A zoom alone is not enough. A quarter turn trades a page's width for its
   * height without necessarily changing the zoom at all, and a raster keyed only
   * by zoom is then considered current: the box turns, the bitmap does not, and
   * CSS stretches the old landscape image over the new portrait box. Which is
   * exactly the skewed page a rotate used to produce.
   */
  drawnFor: string;
  /** The view of the in-flight draw, if any. */
  drawingFor: string;
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
  private mode: PageMode = 'continuous';
  /** The page single-page mode is showing. Meaningless while continuous. */
  private single = 1;
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
  /**
   * Whether pages that already hold a raster are waiting for the view to stop
   * moving before they redraw. Set by a relayout that stretched something;
   * cleared by {@link settle}, which then draws.
   */
  private holding = false;
  private settleTimer: ReturnType<typeof setTimeout> | null = null;

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
    if (this.mode === 'single') return this.single;
    const at = currentPage(
      this.slots.map((slot) => slot.box),
      this.scroll.scrollTop,
      this.scroll.clientHeight,
    );
    return at ?? 1;
  }

  /**
   * Whether the scroller has a box at all.
   *
   * A tab that is not in front is laid out at nothing, and both things this
   * gates on it are wrong to do then: a fit computed against a zero-width
   * scroller is a nonsense zoom that would be written into the layout, and a
   * render started now would stall — pdf.js drives a display render off
   * `requestAnimationFrame`, and a hidden webview runs no frames.
   */
  private get onScreen(): boolean {
    return this.scroll.clientWidth > 0 && this.scroll.clientHeight > 0;
  }

  /** How far into the current page the viewport starts. */
  get offset(): number {
    const slot = this.slots[this.page - 1];
    return slot ? offsetRatio(slot.box, this.scroll.scrollTop) : 0;
  }

  get scale(): number {
    return this.zoom;
  }

  /**
   * Everything about the current view that a raster is only valid for. A slot
   * whose {@link PageSlot.drawnFor} is this is drawn; anything else has to go.
   */
  private get viewKey(): string {
    return `${this.zoom}r${this.rotation}`;
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
   * corrected per page as pages actually render.
   *
   * Deliberately stops short of laying the column out. The caller's next move
   * is to resolve its fit against the page-1 geometry this just measured, and
   * a column that laid itself out first would rasterize a screen of pages at
   * whatever zoom it happened to be carrying only to throw the result away a
   * moment later — wasted work, and a draw racing the one that replaces it.
   */
  async open(doc: PDFDocumentProxy): Promise<void> {
    // Dropping the old document is itself a generation bump, so this one has to
    // be claimed *after* it — the other order hands back a number that is stale
    // before the first `await`, and every open abandons itself.
    this.teardown();
    const generation = ++this.generation;
    this.doc = doc;

    const first = await doc.getPage(1);
    if (generation !== this.generation) return;
    const view = first.getViewport({ scale: 1 });
    this.base = { w: view.width, h: view.height };
    this.geom.set(1, { ...this.base });

    const slots: PageSlot[] = [];
    for (let n = 1; n <= doc.numPages; n += 1) {
      const el = document.createElement('div');
      el.className = 'page placeholder';
      el.dataset.page = String(n);
      el.setAttribute('aria-label', `Page ${n}`);

      const canvas = makeCanvas();

      const text = document.createElement('div');
      text.className = 'page-text';

      const links = document.createElement('div');
      links.className = 'page-links';

      const number = document.createElement('div');
      number.className = 'page-number';
      number.textContent = String(n);

      el.append(canvas, text, links, number);
      slots.push({
        n,
        el,
        canvas,
        text,
        links,
        box: { w: 0, h: 0, top: 0 },
        drawnFor: '',
        drawingFor: '',
        task: null,
        pending: null,
        textDivs: null,
        seq: 0,
      });
    }
    this.slots = slots;
    this.applyFlow();
    this.laidOutFor = '';
  }

  /**
   * Put the slots into the shape the mode asks for.
   *
   * One page per row is the column's own flow, so the slots are its children
   * directly; a spread pairs them inside a `.spread` row, which is what keeps
   * the vertical rhythm — the column's gap between rows — while the row itself
   * owns the gap between the two pages of a pair.
   *
   * Reparenting a slot does not disturb it: a canvas carries its bitmap with
   * it, so a mode switch at the same zoom rearranges what is already drawn
   * rather than rasterizing the visible band again.
   */
  private applyFlow(): void {
    const fragment = document.createDocumentFragment();
    if (this.mode === 'dual') {
      const perRow = columnsFor(this.mode);
      for (let at = 0; at < this.slots.length; at += perRow) {
        const row = document.createElement('div');
        row.className = 'spread';
        for (const slot of this.slots.slice(at, at + perRow)) row.append(slot.el);
        fragment.append(row);
      }
    } else {
      for (const slot of this.slots) fragment.append(slot.el);
    }
    this.host.replaceChildren(fragment);
  }

  /**
   * Set the zoom and rotation, keeping the reader where they were.
   *
   * Always goes on to {@link relayout}, even when neither number moved: the
   * signature guard in there is what decides whether there is anything to do,
   * and it knows about the cases this cannot see — a column that has taken a
   * document but has never been laid out, most of all.
   */
  setView(zoom: number, rotation: Rotation): void {
    this.zoom = zoom;
    this.rotation = rotation;
    this.relayout();
  }

  /** One column, two columns side by side, or one page at a time. */
  setMode(mode: PageMode): void {
    if (mode === this.mode) return;
    // Whichever page the reader was on is the one single mode opens on, and
    // the one the others scroll back to.
    const was = this.page;
    this.mode = mode;
    this.single = clamp(was, 1, Math.max(1, this.slots.length));
    this.applyFlow();
    this.relayout({ force: true });
    if (mode === 'single') {
      this.scroll.scrollTop = 0;
      this.syncViewport();
    } else {
      this.goToPage(was);
    }
  }

  /**
   * Recompute every slot's box and keep the reader in place — the same spot on
   * the same page, not the top of the document.
   *
   * Skipped when nothing about the layout has changed, because a resize that
   * changes neither the scroller nor the zoom — a scrollbar appearing and
   * disappearing — must not start a relayout loop. {@link refresh} is the way
   * past that guard when the boxes are right but the rasters are not.
   */
  relayout(options: { force?: boolean } = {}): void {
    if (this.slots.length === 0 || !this.onScreen) return;
    const view = this.viewSize;
    const signature = `${view.w}x${view.h}@${this.zoom}r${this.rotation}${this.mode}${this.single}`;
    if (!options.force && signature === this.laidOutFor) return;
    this.laidOutFor = signature;

    const anchorIndex = Math.min(this.page, this.slots.length) - 1;
    const anchor = this.slots[anchorIndex]!;
    const within = offsetRatio(anchor.box, this.scroll.scrollTop);

    const boxes = this.computeBoxes();
    const key = this.viewKey;
    let stretched = 0;
    this.slots.forEach((slot, index) => {
      const box = boxes[index]!;
      slot.box = box;
      const shown = this.shows(slot.n);
      // A zero-height flex item still collects the column's gap, so a page
      // single mode is not showing has to leave the flow outright.
      slot.el.style.display = shown ? '' : 'none';
      slot.el.style.width = `${box.w}px`;
      slot.el.style.height = `${box.h}px`;
      if (shown && slot.drawnFor === key) return;
      // Every draw in flight is drawing the wrong view, and a raster from
      // another *turn* is the wrong shape for its box — stretching one of those
      // is exactly the skewed page the rotation half of the key exists to
      // prevent. A raster from another zoom is the right shape at the wrong
      // resolution, so it stays: the stylesheet stretches it over the new box
      // and it reads as a page going soft rather than a page disappearing.
      if (shown && slot.drawnFor !== '' && turnOf(slot.drawnFor) === turnOf(key)) {
        this.stretch(slot);
        stretched += 1;
      } else if (slot.drawnFor !== '' || slot.drawingFor !== '') {
        // A slot holding neither is left alone: on a freshly opened document
        // that is all of them.
        this.release(slot);
      }
    });
    this.scroll.scrollTop = scrollTopAt(this.slots[anchorIndex]!.box, within);
    // Only a relayout with something to show waits. The first layout of a
    // document, and a rotation, have nothing on screen to go soft — holding
    // those back would be a blank page for as long as the wait lasts.
    if (stretched > 0) this.hold();
    this.syncViewport();
  }

  /**
   * Put off redrawing the pages that are currently stretched until the view has
   * stopped moving.
   *
   * Rearmed by every relayout, so a drag-resize rasterizes once, when the
   * reader lets go — not once per frame of the drag, each one cancelling the
   * last before it could reach the screen.
   */
  private hold(): void {
    this.holding = true;
    if (this.settleTimer !== null) clearTimeout(this.settleTimer);
    this.settleTimer = setTimeout(() => this.settle(), SETTLE_MS);
  }

  /** End the wait and draw, whether or not the timer has come round. */
  private settle(): void {
    if (this.settleTimer !== null) clearTimeout(this.settleTimer);
    this.settleTimer = null;
    if (!this.holding) return;
    this.holding = false;
    this.syncViewport();
  }

  /** Every slot's box at the current zoom, rotation and mode. */
  private computeBoxes(): PageBox[] {
    const geoms = this.slots.map((slot) => this.geom.get(slot.n) ?? this.base);
    const scale = viewportScale(this.zoom);
    return this.mode === 'single'
      ? stackSingle(geoms, this.single - 1, scale, this.rotation)
      : stackPages(geoms, scale, this.rotation, columnsFor(this.mode));
  }

  /**
   * Restack after a page turned out not to be the size it was assumed to be,
   * holding the page the reader is looking at where it is on screen.
   *
   * The correction can move any number of boxes — in a spread it can move the
   * page beside it as well as everything below — so the compensation is read
   * off the anchor page rather than computed from one page's delta: if the
   * anchor moved, the scroll moves with it, and if it did not, the reader is
   * already where they should be.
   */
  private restack(): void {
    const boxes = this.computeBoxes();
    const anchorIndex = Math.min(this.page, this.slots.length) - 1;
    const before = this.slots[anchorIndex]?.box.top ?? 0;
    this.slots.forEach((slot, index) => {
      const box = boxes[index]!;
      if (box.w === slot.box.w && box.h === slot.box.h && box.top === slot.box.top) {
        return;
      }
      slot.box = box;
      slot.el.style.width = `${box.w}px`;
      slot.el.style.height = `${box.h}px`;
    });
    const after = this.slots[anchorIndex]?.box.top ?? 0;
    if (after !== before) this.scroll.scrollTop += after - before;
  }

  /**
   * Pick the column's work back up after the tab was away.
   *
   * While it was hidden the webview ran no animation frames, so any page that
   * was drawing when it went is still half-drawn — and since neither the
   * scroller's size nor the zoom changed while it was gone, nothing else here
   * would notice. A draw that is still nominally in flight is dropped rather
   * than waited on: its continuation was scheduled in a frame that never came.
   *
   * Deliberately not a forced relayout. This runs whenever the tab comes back
   * — which is often — and a page that is already drawn correctly must come
   * through it untouched, or every click away and back would cost the whole
   * visible band a re-rasterization.
   */
  refresh(): void {
    if (!this.onScreen) return;
    for (const slot of this.slots) if (slot.drawingFor !== '') this.release(slot);
    this.relayout();
    // A tab coming back to the front is not a gesture to wait out: whatever the
    // resize that hid it left holding, this is the moment to draw it.
    this.settle();
    this.syncViewport();
  }

  /** Scroll a page to the top of the viewport. 1-based. */
  goToPage(page: number): void {
    const slot = this.reveal(page);
    if (!slot) return;
    this.scroll.scrollTop = this.mode === 'single' ? 0 : scrollTopFor(slot.box);
    this.syncViewport();
  }

  /**
   * Scroll to a fraction of the way into a page — the inverse of {@link offset},
   * and how a reopened or reloaded document puts the reader back exactly where
   * they were rather than at the top of the page they were on.
   */
  revealRatio(page: number, ratio: number): void {
    const slot = this.reveal(page);
    if (!slot) return;
    this.scroll.scrollTop = scrollTopAt(slot.box, ratio);
    this.syncViewport();
  }

  /**
   * The anchor a zoom holds still: which page a point in the viewport is over,
   * and how far into it. Paired with {@link holdAnchor}, that is what keeps the
   * spot under the pointer under the pointer across an Alt-wheel zoom.
   */
  anchor(viewportY: number): { page: number; ratio: number } | null {
    return anchorAt(
      this.slots.map((slot) => slot.box),
      this.scroll.scrollTop,
      viewportY,
    );
  }

  /** Put `ratio` of the way into `page` back at `viewportY`. */
  holdAnchor(page: number, ratio: number, viewportY: number): void {
    const slot = this.slots[clamp(page, 1, this.slots.length) - 1];
    if (!slot) return;
    this.scroll.scrollTop = Math.max(0, slot.box.top + ratio * slot.box.h - viewportY);
    this.syncViewport();
  }

  /**
   * Scroll so a point in PDF user space is a third of the way down the
   * viewport — where a jumped-to destination reads as arrived at rather than
   * as clipped to the top edge.
   */
  revealPoint(page: number, point: { x: number; y: number } | null): void {
    if (!point) {
      this.goToPage(page);
      return;
    }
    const slot = this.reveal(page);
    if (!slot) return;
    const base = this.geom.get(slot.n) ?? this.base;
    const on = pointOnPage(base, this.rotation, point, viewportScale(this.zoom));
    this.scroll.scrollTop = Math.max(
      0,
      slot.box.top + on.y - this.scroll.clientHeight / 3,
    );
    this.syncViewport();
  }

  /**
   * Ensure a page holds a raster before something is measured against it.
   *
   * In single-page mode a page that is not the one on screen has no box to
   * draw into, so this brings it forward first — which is what lets find step
   * to a match on another page without the caller knowing which mode it is in.
   */
  async ensureDrawn(page: number): Promise<void> {
    const slot = this.reveal(page);
    if (!slot) return;
    // A caller that needs the page *now* — find stepping to a match, an export
    // — is not waiting out a gesture, and a stretched raster is not the page at
    // this zoom. Ending the wait draws the rest of the band with it.
    this.settle();
    await this.draw(slot);
  }

  /** Whether a page has a box in the current mode. */
  private shows(page: number): boolean {
    return this.mode !== 'single' || page === this.single;
  }

  /**
   * The slot for a page, brought into the flow first if single-page mode was
   * showing a different one. Null when the page is not in the document.
   */
  private reveal(page: number): PageSlot | undefined {
    if (this.slots.length === 0) return undefined;
    const target = clamp(page, 1, this.slots.length);
    if (this.mode === 'single' && this.single !== target) {
      this.single = target;
      this.relayout({ force: true });
    }
    return this.slots[target - 1];
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
    // Nothing is drawn for a tab that is not in front: pdf.js continues a
    // display render on an animation frame, and a hidden webview is given
    // none — so a draw started now stops part-way and stays that way. `refresh`
    // is what picks the work up when the tab comes back.
    if (this.slots.length === 0 || !this.onScreen) return;
    const top = this.scroll.scrollTop;
    const view = this.scroll.clientHeight;
    for (const slot of this.slots) {
      if (this.shows(slot.n) && isWithinBand(slot.box, top, view, this.options.renderAhead)) {
        void this.draw(slot);
      } else if (slot.drawnFor !== '' || slot.drawingFor !== '') {
        // Scrolling away is the one release that has to spare a live selection:
        // the boxes it is anchored in are still where the reader put them, and
        // dropping them mid-drag would collapse a selection that started on the
        // page above. A relayout releases without the reprieve, because there
        // the boxes have genuinely moved.
        this.release(slot, { sparingSelection: true });
      }
    }
    const at =
      this.mode === 'single'
        ? this.single
        : currentPage(
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
   * Idempotent per view, so the sync pass can call it for every visible slot on
   * every frame — and a caller that needs the page *now* (find, export) gets
   * the draw already in flight rather than a second one.
   */
  private draw(slot: PageSlot): Promise<void> {
    const key = this.viewKey;
    if (slot.drawnFor === key) return Promise.resolve();
    // A page that is showing a stretched raster has something to look at, so it
    // waits for the view to stop moving. A page that is showing nothing does
    // not: a scroll during a resize still has to fill the band ahead of it.
    if (this.holding && slot.drawnFor !== '') return Promise.resolve();
    if (slot.drawingFor === key && slot.pending) return slot.pending;
    slot.pending = this.drawNow(slot, this.zoom, this.rotation, key);
    return slot.pending;
  }

  /**
   * The view is passed in rather than read back off the column: a draw outlives
   * several awaits, and the zoom and rotation it was started for are what its
   * viewport, its text layer and its link boxes all have to agree on.
   */
  private async drawNow(
    slot: PageSlot,
    zoom: number,
    rotation: Rotation,
    key: string,
  ): Promise<void> {
    const doc = this.doc;
    if (!doc) return;
    slot.drawingFor = key;
    const seq = ++slot.seq;
    const generation = this.generation;
    const stale = (): boolean => generation !== this.generation || seq !== slot.seq;
    /** The surface this draw paints into, until the slot adopts it. */
    let painting: HTMLCanvasElement | null = null;

    try {
      const proxy = await doc.getPage(slot.n);
      if (stale()) return;
      const unrotated = proxy.getViewport({ scale: 1, rotation: proxy.rotate });
      this.noteGeometry(slot, unrotated.width, unrotated.height);

      const viewport = proxy.getViewport({
        scale: viewportScale(zoom),
        rotation: (proxy.rotate + rotation) % 360,
      });
      const ratio = rasterRatio(
        slot.box,
        window.devicePixelRatio || 1,
        this.options.maxCanvasPixels,
      );
      // A render still running belongs to an attempt this one has superseded,
      // and nothing it paints will ever be shown — it is stopped rather than
      // left to compete for the main thread with the draw replacing it.
      slot.task?.cancel();

      // Off to the side, never into the canvas the reader is looking at: sizing
      // a canvas clears it, so drawing in place would blank the page for the
      // whole of the render and flash it back at the end. This one takes the
      // old one's place already finished.
      //
      // Only the bitmap is sized here — the canvas and the layers over it are
      // stretched across the slot by CSS, so nothing they carry can outlive the
      // raster and stretch the scroll extent.
      const canvas = makeCanvas();
      painting = canvas;
      canvas.width = Math.max(1, Math.round(viewport.width * ratio));
      canvas.height = Math.max(1, Math.round(viewport.height * ratio));
      slot.task = proxy.render({
        canvas,
        viewport,
        transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0],
      });
      await slot.task.promise;
      if (stale()) return;
      slot.canvas.replaceWith(canvas);
      discard(slot.canvas);
      slot.canvas = canvas;
      slot.el.classList.remove('placeholder');

      if (this.options.textLayer) {
        // The text layer is what makes the page selectable, findable and
        // readable by a screen reader; without it a page is a picture of words.
        slot.text.replaceChildren();
        // What every run's size is computed from: pdf.js writes each run's
        // height in PDF units and leaves the stylesheet to multiply it by this.
        // `--total-scale-factor` is the name it took in pdf.js 5; under the old
        // `--scale-factor` nothing read it, every run fell back to the font it
        // inherited, and a selection landed nowhere near the words it covered.
        // The viewport's own scale, not the zoom: it is what the runs were laid
        // out against.
        slot.text.style.setProperty('--total-scale-factor', String(viewport.scale));
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

      if (this.options.links) await this.drawLinks(slot, proxy, zoom, rotation, stale);

      slot.drawnFor = key;
    } catch {
      // A cancelled render (scrolled away, rezoomed) or a page that will not
      // rasterize is not a dead document — the slot stays blank at the right
      // size and the next pass over it retries.
    } finally {
      // A surface the slot never adopted — a cancelled render, a page that
      // would not rasterize — is nowhere in the DOM and holds a bitmap the size
      // of a page. It is handed back here rather than left to the collector.
      if (painting !== null && painting !== slot.canvas) discard(painting);
      // Only the draw that is still the slot's own clears the slot's in-flight
      // bookkeeping. An older attempt finishing late used to report that no
      // draw was running while one was, which cost `draw` its one guarantee —
      // that a slot has at most one draw in flight — and put two pdf.js render
      // tasks on one canvas. Each then wiped the other: what the reader got was
      // a page under its placeholder mask, or a black one with a few runs of
      // text on it, and nothing to fix it until a scroll or a resize.
      if (!stale()) {
        slot.task = null;
        slot.drawingFor = '';
        slot.pending = null;
      }
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
    zoom: number,
    rotation: Rotation,
    stale: () => boolean,
  ): Promise<void> {
    const annotations = await proxy.getAnnotations({ intent: 'display' });
    if (stale()) return;
    slot.links.replaceChildren();

    const base = this.geom.get(slot.n) ?? this.base;
    const scale = viewportScale(zoom);
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
      const a = pointOnPage(base, rotation, { x: x1, y: y1 }, scale);
      const b = pointOnPage(base, rotation, { x: x2, y: y2 }, scale);

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

  /**
   * Drop a slot's raster and layers, keeping its box.
   *
   * `sparingSelection` leaves the text layer standing when the reader's
   * selection reaches into it — the glyph boxes are invisible and cost nothing
   * to keep, and they are the nodes the selection is anchored in. The raster
   * goes either way, which is what the memory ceiling is actually about.
   */
  private release(slot: PageSlot, options: { sparingSelection?: boolean } = {}): void {
    slot.seq += 1;
    slot.task?.cancel();
    slot.task = null;
    slot.pending = null;
    slot.drawnFor = '';
    slot.drawingFor = '';
    discard(slot.canvas);
    if (!(options.sparingSelection === true && holdsSelection(slot.text))) {
      slot.text.replaceChildren();
      slot.textDivs = null;
    }
    slot.links.replaceChildren();
    slot.el.classList.add('placeholder');
  }

  /**
   * Keep a slot's raster across a relayout, stretched over its new box.
   *
   * Releasing instead is what made a drag-resize flicker: the boxes change on
   * every frame of the drag, so every frame blanked the visible pages down to
   * their placeholder and started a render that the next frame cancelled. The
   * reader spent the whole gesture looking at empty grey rectangles.
   *
   * The bitmap is the one thing worth keeping. Its resolution is now wrong —
   * the stylesheet sizes the canvas to the slot, so the page reads as soft
   * until the draw that replaces it lands — but its *shape* is not, and a soft
   * page is a page. Everything over it goes: the text runs and the link boxes
   * are laid out for the old scale, and runs left standing would paint the
   * reader's selection nowhere near the words it covers.
   */
  private stretch(slot: PageSlot): void {
    slot.seq += 1;
    slot.task?.cancel();
    slot.task = null;
    slot.pending = null;
    slot.drawingFor = '';
    slot.text.replaceChildren();
    slot.textDivs = null;
    slot.links.replaceChildren();
  }

  /**
   * A page turned out not to be page 1's size. Correct its box and slide the
   * pages below it, without moving what the reader is looking at.
   */
  private noteGeometry(slot: PageSlot, w: number, h: number): void {
    const previous = this.geom.get(slot.n);
    if (previous && previous.w === w && previous.h === h) return;
    this.geom.set(slot.n, { w, h });
    // Single mode has one page in the flow: a correction to any other one is
    // recorded for when that page comes forward, and changes nothing now.
    if (this.mode === 'single' && slot.n !== this.single) return;
    this.restack();
  }

  private teardown(): void {
    this.generation += 1;
    if (this.settleTimer !== null) clearTimeout(this.settleTimer);
    this.settleTimer = null;
    this.holding = false;
    for (const slot of this.slots) this.release(slot);
    this.slots = [];
    this.geom.clear();
    this.textCache.clear();
    this.reported = 0;
    this.single = 1;
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

/** A page's raster surface. The stylesheet stretches it across the slot. */
function makeCanvas(): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.className = 'page-canvas';
  canvas.setAttribute('aria-hidden', 'true');
  return canvas;
}

/**
 * Give a canvas's bitmap back.
 *
 * A page of a big document at a deep zoom is tens of megabytes, and dropping
 * the element is not enough on its own — the backing store lives until the
 * collector gets to it, and a virtualized column drops canvases faster than
 * that. Sizing it to nothing frees it now.
 */
function discard(canvas: HTMLCanvasElement): void {
  canvas.width = 0;
  canvas.height = 0;
}

/**
 * The rotation half of a view key — the half a stretched raster cannot survive.
 *
 * A quarter turn trades a page's width for its height, so a raster from another
 * turn stretched over the new box is the skewed page {@link PageSlot.drawnFor}
 * exists to prevent. A raster from another zoom is the same picture at another
 * size, which is exactly what stretching it means.
 */
function turnOf(key: string): string {
  return key.slice(key.indexOf('r') + 1);
}

/**
 * Whether the reader's selection reaches into an element.
 *
 * Asked of the element's own root rather than of the document: the viewer lives
 * in a shadow root, and `document.getSelection()` retargets to the host element
 * there — it would answer for the `<pdf-viewer>` tag, never for a page's text.
 * `ShadowRoot.getSelection` is Chromium's, which is the only engine a VSCode
 * webview runs in; the document is the fallback for anything else, tests
 * included.
 */
function holdsSelection(element: HTMLElement): boolean {
  const root = element.getRootNode() as { getSelection?: () => Selection | null };
  const selection =
    typeof root.getSelection === 'function' ? root.getSelection() : document.getSelection();
  if (!selection || selection.isCollapsed) return false;
  for (let index = 0; index < selection.rangeCount; index += 1) {
    const range = selection.getRangeAt(index);
    // Not every DOM implementation the tests run against has it.
    if (typeof range.intersectsNode !== 'function') return false;
    if (range.intersectsNode(element)) return true;
  }
  return false;
}
