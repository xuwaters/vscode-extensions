import { FASTElement, Updates, css, customElement, observable } from '@microsoft/fast-element';
import type { PDFDocumentProxy } from 'pdfjs-dist';
import type {
  DocumentSource,
  FitMode,
  HostToWebview,
  PageMode,
  PdfAssetUrls,
  Rotation,
  ViewerCommand,
  ViewerPlace,
  ViewerSettings,
  WebviewToHost,
} from '../../src/messages.js';
import { ChunkBuffer } from '../model/chunks.js';
import { clampZoom, columnsFor, fitZoom, stepZoom } from '../model/layout.js';
import type { OutlineRow, RawOutlineItem } from '../model/outline.js';
import { formatZoomPercent, parseZoomPercent } from '../model/zoom.js';
import { resolveDestination } from '../render/destinations.js';
import { PageColumn, type LinkTarget } from '../render/pageColumn.js';
import { bootWorker, documentParams, pdfjs } from '../render/pdfjs.js';
import { JumpHistory } from './jumpHistory.js';
import { OutlineState } from './outlineState.js';
import { Search } from './search.js';
import sheet from './styles.css';
import { template } from './template.js';

/** How long a transient notice stays on screen. */
const NOTICE_MS = 2400;

/** How long the pointer rests on a toolbar control before it explains itself. */
const TIP_MS = 350;

/** How far under a control its tooltip sits. */
const TIP_GAP = 4;

/** The resolution an exported page is rendered at, relative to actual size. */
const EXPORT_ZOOM = 2;

/**
 * How hard a wheel notch pushes the zoom.
 *
 * Applied as `exp(-delta * WHEEL_ZOOM)`, so the zoom is continuous rather than
 * the toolbar's discrete steps and a notch costs the same fraction whichever
 * end of the range it is at. A trackpad sends many small deltas and a mouse
 * sends few large ones; the exponent is what makes both feel the same.
 */
const WHEEL_ZOOM = 0.0022;

/** Line- and page-mode wheel deltas, in the pixels they stand for. */
const WHEEL_LINE_PX = 16;
const WHEEL_PAGE_PX = 400;

/** What the element needs from the extension host. */
export interface ViewerHost {
  post(message: WebviewToHost): void;
}

const styles = css`
  ${sheet}
`;

/**
 * `<pdf-viewer>` — the whole viewer.
 *
 * The split is the one `mc-pdf-viewer` uses and for the same reason: the
 * *chrome* is reactive and lives in the template, and the *pages* are not.
 * A page column is hundreds of boxes of which a handful hold a canvas at any
 * moment, rasterized and released as the reader scrolls; expressing that as
 * bindings would mean a binding per page and a canvas per binding. So
 * `PageColumn` owns the column imperatively and this element owns everything
 * the reader can see the state of.
 */
/**
 * The tag, named once. `@customElement` in fast-element 3 defines the element
 * *asynchronously* — `compose` resolves a promise before `customElements.define`
 * is ever called — so the bootstrap has to wait on this name rather than
 * construct the class the moment the module has evaluated.
 */
export const PDF_VIEWER_TAG = 'pdf-viewer';

@customElement({ name: PDF_VIEWER_TAG, template, styles })
export class PdfViewer extends FASTElement {
  /** Set by the bootstrap before the element is attached. */
  host!: ViewerHost;

  @observable state: 'idle' | 'loading' | 'ready' | 'error' = 'idle';
  @observable name = 'document.pdf';
  @observable loadingHint = 'Opening…';
  @observable errorMessage = '';
  @observable notice = '';
  @observable progress = 0;

  @observable page = 1;
  @observable pageCount = 0;
  @observable pageField = '1';
  @observable zoom = 1;
  @observable zoomField = '100%';
  @observable fit: FitMode = 'fit-width';
  @observable mode: PageMode = 'continuous';
  @observable rotation: Rotation = 0;
  @observable inverted = false;

  @observable outlineVisible = false;
  @observable outlineWidth = 240;

  /** What the toolbar's tooltip says, or null while nothing is explaining itself. */
  @observable tip: { text: string; keys: string } | null = null;

  /**
   * The controllers the chrome binds through. All are constant references
   * holding observables of their own, so a template that reads
   * `x.outline.shown` or `x.search.label` tracks the observable it actually
   * read — see `outlineState.ts` on why that matters for the twisty.
   */
  readonly outline = new OutlineState();
  /** Where each jump came from, so the toolbar can undo one. */
  readonly history = new JumpHistory();
  readonly search = new Search({
    column: () => this.column,
    pageCount: () => this.pageCount,
    page: () => this.page,
    reveal: (range) => this.revealRange(range),
  });

  @observable settings: ViewerSettings = {
    defaultZoom: 'fit-width',
    background: 'editor',
    invertColors: 'never',
    textLayer: true,
    links: true,
    outlineVisible: false,
    outlineWidth: 240,
    maxCanvasPixels: 16 << 20,
    renderAhead: 1,
  };

  /** Bound by the template. */
  scrollEl!: HTMLElement;
  columnEl!: HTMLElement;
  findInput!: HTMLInputElement;
  pageInput!: HTMLInputElement;
  zoomInput!: HTMLInputElement;
  tipEl!: HTMLElement;

  private assets: PdfAssetUrls | null = null;
  private doc: PDFDocumentProxy | null = null;
  private column: PageColumn | null = null;
  private source: DocumentSource | null = null;
  /** Where to put the reader once the document is open. One-shot. */
  private pendingRestore: ViewerPlace | undefined;
  private readonly incoming = new ChunkBuffer();
  /** The generation the slices currently in `incoming` were asked for by. */
  private bytesFor = -1;
  /** Invalidates async work when the document is swapped or torn down. */
  private generation = 0;

  private noticeTimer: ReturnType<typeof setTimeout> | undefined;
  private placeTimer: ReturnType<typeof setTimeout> | undefined;

  /** The control the tooltip is describing, and the wait before it appears. */
  private tipAnchor: HTMLElement | null = null;
  private tipTimer: ReturnType<typeof setTimeout> | undefined;

  private resizeObserver: ResizeObserver | null = null;
  private resizeAttached: HTMLElement | null = null;
  private resizePointer: { id: number; startX: number; startWidth: number } | null = null;
  /** Whether the last thing the resize observer saw was a tab with no box. */
  private wasOffScreen = false;

  /**
   * Alt-wheel zooms, about the pointer.
   *
   * Only Alt. Ctrl- and Cmd-wheel belong to the editor around us, and this is
   * the one modifier nothing else in a VSCode window claims. Nor are the
   * keyboard shortcuts handled here: VSCode forwards every keystroke a webview
   * sees to its own keybinding resolver whatever the page does with it, so a
   * shortcut answered in both places is answered twice. They arrive as
   * `command` messages instead — which is also what makes them rebindable.
   */
  private readonly onWheel = (event: WheelEvent): void => {
    if (!event.altKey || event.ctrlKey || event.metaKey) return;
    if (this.state !== 'ready') return;
    event.preventDefault();
    this.zoomAbout(this.zoom * Math.exp(-wheelPixels(event) * WHEEL_ZOOM), event);
  };

  override connectedCallback(): void {
    super.connectedCallback();
    // Not passive: the whole point is to take the scroll away and zoom instead.
    window.addEventListener('wheel', this.onWheel, { passive: false });
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    window.removeEventListener('wheel', this.onWheel);
    this.teardown();
  }

  // ── The host protocol ──────────────────────────────────────────────────────

  /** Everything the extension host says arrives here. */
  handle(message: HostToWebview): void {
    switch (message.type) {
      case 'open':
        this.name = message.name;
        this.assets = message.assets;
        this.applySettings(message.settings, message.restore === undefined);
        this.pendingRestore = message.restore;
        if (message.restore) this.applyPlace(message.restore);
        void this.load(message.source);
        break;

      case 'reload':
        void this.load(message.source, { keepPlace: true });
        break;

      case 'chunk':
        this.onChunk(message.index, message.total, message.data);
        break;

      case 'settings':
        this.applySettings(message.settings, false);
        break;

      case 'command':
        void this.runCommand(message.command, message.page);
        break;

      case 'visible':
        this.revalidate();
        break;

      case 'hostError':
        this.fail(message.message);
        break;
    }
  }

  private async runCommand(command: ViewerCommand, page?: number): Promise<void> {
    switch (command) {
      case 'nextPage':
        this.goToPage(this.page + 1);
        break;
      case 'previousPage':
        this.goToPage(this.page - 1);
        break;
      case 'goToPage':
        if (page !== undefined) this.jumpToPage(page);
        break;
      case 'focusPage':
        this.focusPage();
        break;
      case 'goBack':
        this.goBack();
        break;
      case 'zoomIn':
        this.zoomBy(1);
        break;
      case 'zoomOut':
        this.zoomBy(-1);
        break;
      case 'zoomReset':
        this.applyFit('actual');
        break;
      case 'fitWidth':
        this.applyFit('fit-width');
        break;
      case 'fitPage':
        this.applyFit('fit-page');
        break;
      case 'fitHeight':
        this.applyFit('fit-height');
        break;
      case 'singlePage':
        this.applyPageMode('single');
        break;
      case 'continuousPages':
        this.applyPageMode('continuous');
        break;
      case 'dualPages':
        this.applyPageMode('dual');
        break;
      case 'rotateClockwise':
        this.rotateBy(1);
        break;
      case 'rotateCounterclockwise':
        this.rotateBy(-1);
        break;
      case 'toggleOutline':
        this.toggleOutline();
        break;
      case 'toggleInvertColors':
        this.toggleInvert();
        break;
      case 'find':
        this.focusFind();
        break;
      case 'exportPagePng':
        await this.exportPage();
        break;
    }
  }

  // ── Loading ────────────────────────────────────────────────────────────────

  private async load(
    source: DocumentSource,
    options: { keepPlace?: boolean } = {},
  ): Promise<void> {
    this.source = source;
    // A reload puts the reader back where they were; a first open puts them
    // where the host remembers them from last time, if it remembers anything.
    if (options.keepPlace) this.pendingRestore = this.currentPlace();
    const generation = ++this.generation;

    if (source.kind === 'bytes') {
      // The bytes arrive over the message channel, chunk by chunk; the page
      // asks for them and `onChunk` resumes this.
      this.incoming.reset();
      this.bytesFor = generation;
      this.progress = 0;
      this.loadingHint = 'Receiving the document…';
      if (!options.keepPlace) this.state = 'loading';
      this.host.post({ type: 'needBytes', reason: 'no resource URL for this document' });
      return;
    }

    if (!options.keepPlace) {
      this.state = 'loading';
      this.loadingHint = 'Opening…';
      this.progress = 0;
    }

    try {
      await this.openDocument({ url: source.url }, generation);
      if (options.keepPlace && generation === this.generation) this.say('Reloaded');
    } catch (error) {
      if (generation !== this.generation) return;
      // A resource URL that will not load is the one failure worth retrying a
      // different way: the host can still put the bytes on the wire.
      this.host.post({ type: 'needBytes', reason: describe(error) });
    }
  }

  /**
   * One slice of the byte fallback.
   *
   * Slices in flight when the document is swapped belong to the request that
   * asked for them, not to this one — mixing the two would assemble a file that
   * is neither.
   */
  private onChunk(index: number, total: number, data: string): void {
    if (this.bytesFor !== this.generation) return;
    const generation = this.generation;
    let bytes: Uint8Array | null;
    try {
      bytes = this.incoming.add(index, total, data);
    } catch (error) {
      this.incoming.reset();
      this.fail(`The document could not be decoded: ${describe(error)}`);
      return;
    }
    this.progress = this.incoming.progress;
    if (!bytes) return;

    this.loadingHint = 'Opening…';
    void this.openDocument({ data: bytes }, generation).catch((error: unknown) => {
      if (generation === this.generation) this.fail(describe(error));
    });
  }

  private async openDocument(
    source: { url: string } | { data: Uint8Array },
    generation: number,
  ): Promise<void> {
    const assets = this.assets;
    if (!assets) throw new Error('the host did not say where pdf.js lives');
    await bootWorker(assets.worker);
    if (generation !== this.generation) return;

    const task = pdfjs.getDocument(documentParams(source, assets));
    task.onProgress = ({ loaded, total }: { loaded: number; total: number }) => {
      if (generation === this.generation && total > 0) this.progress = loaded / total;
    };
    const doc = await task.promise;
    if (generation !== this.generation) {
      void doc.loadingTask.destroy().catch(() => {});
      return;
    }

    const previous = this.doc;
    this.doc = doc;
    this.pageCount = doc.numPages;
    // A new document has nothing to do with the last one's results, and the
    // places it was jumped from describe pages that are no longer there.
    this.search.reset();
    this.history.clear();
    this.state = 'ready';

    // The `ready` branch of the template mounts on the next update — the page
    // column cannot be built before its host exists.
    await Updates.next();
    if (generation !== this.generation) return;
    this.mountColumn();
    await this.column?.open(doc);
    // Only now: until the column has taken the new document it is still
    // rendering pages out of the old one, and destroying that task under it
    // turns a clean swap into a burst of rejected renders.
    void previous?.loadingTask.destroy().catch(() => {});
    if (generation !== this.generation) return;

    const place = this.pendingRestore;
    this.pendingRestore = undefined;
    this.applyZoom();
    if (place) this.restore(place);
    else this.goToPage(1, { report: false });

    void this.loadOutline(doc, generation);
    this.host.post({ type: 'opened', pageCount: doc.numPages });
    this.reportPlace();
  }

  /** Rebuild the column against the template's freshly mounted scroller. */
  private mountColumn(): void {
    if (!this.column || this.resizeAttached !== this.scrollEl) {
      this.column?.destroy();
      this.column = new PageColumn(
        this.scrollEl,
        this.columnEl,
        {
          onPage: (page) => this.onPageChanged(page),
          onLink: (target) => void this.followLink(target),
          onTextLayer: () => this.search.repaint(),
        },
        this.columnOptions(),
      );
      this.observeResize();
    }
    // Before the document is taken, so the first layout is already the right
    // shape rather than a continuous column that flickers into a single page.
    this.column.setMode(this.mode);
  }

  private columnOptions() {
    return {
      textLayer: this.settings.textLayer,
      links: this.settings.links,
      maxCanvasPixels: this.settings.maxCanvasPixels,
      renderAhead: this.settings.renderAhead,
    };
  }

  private async loadOutline(doc: PDFDocumentProxy, generation: number): Promise<void> {
    let raw: RawOutlineItem[] | null = null;
    try {
      raw = (await doc.getOutline()) as RawOutlineItem[] | null;
    } catch {
      raw = null;
    }
    if (generation !== this.generation) return;
    this.outline.load(raw);

    // The page each entry points at, resolved lazily and in the background: a
    // 2000-entry outline is 2000 destination lookups, and the sidebar is usable
    // — clickable, scrollable — before a single one of them lands. They only
    // decide which row is highlighted as the reader scrolls.
    for (const [index, row] of this.outline.all.entries()) {
      if (generation !== this.generation) return;
      this.outline.setPage(index, (await resolveDestination(doc, row.dest))?.page);
    }
    if (generation === this.generation) this.outline.markPage(this.page);
  }

  private fail(message: string): void {
    this.state = 'error';
    this.errorMessage = message;
  }

  /** The Try Again button: whatever we were told to open, again. */
  retry(): void {
    if (this.source) void this.load(this.source);
  }

  private teardown(): void {
    this.generation += 1;
    this.search.dispose();
    if (this.noticeTimer) clearTimeout(this.noticeTimer);
    if (this.placeTimer) clearTimeout(this.placeTimer);
    if (this.tipTimer) clearTimeout(this.tipTimer);
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    this.column?.destroy();
    this.column = null;
    void this.doc?.loadingTask.destroy().catch(() => {});
    this.doc = null;
  }

  // ── Settings and place ─────────────────────────────────────────────────────

  private applySettings(settings: ViewerSettings, adoptDefaults: boolean): void {
    const before = this.settings;
    this.settings = settings;
    // The toolbar's toggle is the reader's, and a settings push must not undo
    // it — only a *change* to the setting overrides what they chose.
    if (adoptDefaults || before.invertColors !== settings.invertColors) {
      this.inverted =
        settings.invertColors === 'always' ||
        (settings.invertColors === 'auto' && prefersDark());
    }

    if (adoptDefaults) {
      this.fit = settings.defaultZoom;
      this.outlineVisible = settings.outlineVisible;
      this.outlineWidth = settings.outlineWidth;
    }
    this.column?.setOptions(this.columnOptions());
    this.applyZoom();
  }

  private applyPlace(place: ViewerPlace): void {
    this.page = place.page;
    this.zoom = clampZoom(place.zoom);
    this.fit = place.fit;
    this.mode = place.mode;
    this.column?.setMode(place.mode);
    this.rotation = place.rotation;
    this.inverted = place.inverted;
    this.outlineVisible = place.outlineVisible;
    this.outlineWidth = place.outlineWidth;
    this.showPage();
    this.showZoom();
  }

  private restore(place: ViewerPlace): void {
    this.applyPlace(place);
    this.applyZoom();
    // `offsetRatio` is a fraction of the *page's* height, which is why this
    // goes through the column: only it knows how tall the page ended up.
    this.column?.revealRatio(place.page, place.offsetRatio);
  }

  private currentPlace(): ViewerPlace {
    return {
      page: this.page,
      zoom: this.zoom,
      fit: this.fit,
      mode: this.mode,
      rotation: this.rotation,
      inverted: this.inverted,
      outlineVisible: this.outlineVisible,
      outlineWidth: this.outlineWidth,
      offsetRatio: this.column?.offset ?? 0,
    };
  }

  /** Tell the host where the reader stands, at most a few times a second. */
  private reportPlace(): void {
    if (this.placeTimer) clearTimeout(this.placeTimer);
    this.placeTimer = setTimeout(() => {
      this.placeTimer = undefined;
      this.host.post({ type: 'place', place: this.currentPlace() });
    }, 200);
  }

  // ── Zoom, rotation, navigation ─────────────────────────────────────────────

  /**
   * Resolve the current fit into a scale and hand it to the column.
   *
   * A tab with no box is left alone: a fit measured against a zero-width
   * scroller resolves to the bottom of the zoom range, and writing that into
   * the layout is how a document comes back from the background at 10%.
   */
  private applyZoom(): void {
    const column = this.column;
    if (!column) return;
    const view = column.viewSize;
    if (this.fit !== 'actual' && (view.w === 0 || view.h === 0)) return;
    const zoom =
      this.fit === 'actual'
        ? this.zoom
        : fitZoom(this.fit, view, column.baseGeom, columnsFor(this.mode), this.rotation);
    this.zoom = zoom;
    this.showZoom();
    column.setView(zoom, this.rotation);
  }

  applyFit(fit: FitMode): void {
    this.fit = fit;
    if (fit === 'actual') this.zoom = 1;
    this.applyZoom();
    this.reportPlace();
  }

  /** One column, two columns side by side, or one page at a time. */
  applyPageMode(mode: PageMode): void {
    if (this.mode === mode) return;
    this.mode = mode;
    this.column?.setMode(mode);
    // A fit is measured per column, so the same fit is a different zoom in a
    // spread than it is in a single column — and the column has restacked, so
    // the page the reader is on may have changed with it.
    this.applyZoom();
    this.page = this.column?.page ?? this.page;
    this.showPage();
    this.reportPlace();
  }

  togglePageMode(): void {
    this.applyPageMode(this.mode === 'single' ? 'continuous' : 'single');
  }

  /** The toolbar's spread toggle: two pages side by side, or back to one. */
  toggleDualMode(): void {
    this.applyPageMode(this.mode === 'dual' ? 'continuous' : 'dual');
  }

  zoomBy(direction: 1 | -1): void {
    this.fit = 'actual';
    this.zoom = stepZoom(this.zoom, direction);
    this.applyZoom();
    this.reportPlace();
  }

  setZoom(zoom: number): void {
    this.fit = 'actual';
    this.zoom = clampZoom(zoom);
    this.applyZoom();
    this.reportPlace();
  }

  /**
   * Zoom while holding the spot under the pointer still.
   *
   * Without the anchor a wheel zoom walks away from whatever the reader was
   * looking at, because the column's own relayout holds the *top of the
   * viewport* in place — which is the right answer for the toolbar's buttons
   * and the wrong one for a pointer that is halfway down the page.
   */
  private zoomAbout(zoom: number, at: { clientX: number; clientY: number }): void {
    const column = this.column;
    const el = this.scrollEl;
    const next = clampZoom(zoom);
    if (!column || !el || next === this.zoom) return;

    const rect = el.getBoundingClientRect();
    const x = at.clientX - rect.left;
    const y = at.clientY - rect.top;
    const anchor = column.anchor(y);
    // Horizontal has no page to anchor to — the column is one centred stack,
    // so the offset simply scales with the zoom.
    const factor = next / this.zoom;
    const left = (el.scrollLeft + x) * factor - x;

    this.fit = 'actual';
    this.zoom = next;
    this.applyZoom();
    if (anchor) column.holdAnchor(anchor.page, anchor.ratio, y);
    el.scrollLeft = Math.max(0, left);
    this.reportPlace();
  }

  rotateBy(direction: 1 | -1): void {
    this.rotation = ((((this.rotation + direction * 90) % 360) + 360) % 360) as Rotation;
    // A quarter turn trades a page's width for its height, so a fit means
    // something different than it did a moment ago — and the boxes have to be
    // restacked, and every page on screen rasterized again, even when the zoom
    // lands on the same number. `setView` after the fit is what covers the tab
    // that is not in front: `applyZoom` leaves a fit alone there, and the column
    // still has to be told which way up it will be when it comes back.
    this.applyZoom();
    this.column?.setView(this.zoom, this.rotation);
    this.reportPlace();
  }

  /**
   * The Go to Page command: the toolbar's own page box, selected and ready to
   * be typed over.
   *
   * Not a host input box. The number is already on screen, beside the page
   * count and the buttons that step through it — asking for it in a quick-pick
   * over the top of the document puts the answer somewhere the reader is not
   * looking, and leaves the box that says the same thing inert.
   */
  focusPage(): void {
    this.pageInput?.focus();
    this.pageInput?.select();
  }

  goToPage(page: number, options: { report?: boolean } = {}): void {
    const target = Math.min(Math.max(1, Math.round(page)), Math.max(1, this.pageCount));
    this.page = target;
    this.showPage();
    this.column?.goToPage(target);
    if (options.report !== false) this.reportPlace();
  }

  /**
   * Go to a page the way the page box and the palette do: as a jump, which is
   * something the reader can come back from. Turning the page is not one —
   * a back button that undid a single page turn would be the previous-page
   * button with a longer name.
   */
  jumpToPage(page: number): void {
    this.recordJump();
    this.goToPage(page);
  }

  /** Remember where the reader stands, before something takes them away. */
  private recordJump(): void {
    this.history.push({ page: this.page, offsetRatio: this.column?.offset ?? 0 });
  }

  /**
   * Undo the last jump.
   *
   * Back to the exact spot, not to the top of the page it was on: a footnote
   * link followed from halfway down a page has to come back to the sentence it
   * was in, or the button has lost the reader's place while claiming to keep it.
   */
  goBack(): void {
    const at = this.history.pop();
    if (!at) return;
    this.page = Math.min(at.page, Math.max(1, this.pageCount));
    this.showPage();
    this.column?.revealRatio(this.page, at.offsetRatio);
    this.reportPlace();
  }

  private onPageChanged(page: number): void {
    this.page = page;
    // Not while the reader is typing in it: following the scroll would rewrite
    // the number half-way through the one they are entering.
    if (this.shadowRoot?.activeElement?.classList.contains('field-input') !== true) {
      this.showPage();
    }
    this.outline.markPage(this.page);
    this.reportPlace();
  }

  /**
   * Write the page back into its box.
   *
   * Both of these set the element's value as well as the observable. A binding
   * only pushes when the value it reads *changes*, and the case that matters
   * most is exactly the one where it does not: the reader typed something
   * unreadable into the box, the correction is the number that was already
   * there, and without this the box keeps showing the nonsense.
   */
  showPage(): void {
    this.pageField = String(this.page);
    if (this.pageInput) this.pageInput.value = this.pageField;
  }

  showZoom(): void {
    this.zoomField = formatZoomPercent(this.zoom);
    if (this.zoomInput) this.zoomInput.value = this.zoomField;
  }

  /**
   * A page number typed into the box.
   *
   * Out of range is not a typo: 0 and 900 in a 300-page document both name an
   * end of it, so they go there rather than being thrown away — {@link goToPage}
   * clamps. Only text that is not a number at all leaves the reader where they
   * are, with the box put back to the page they are on.
   */
  onPageEntered(event: Event): void {
    const text = (event.target as HTMLInputElement).value.trim();
    const value = Number(text);
    if (text !== '' && Number.isFinite(value)) this.jumpToPage(value);
    else this.showPage();
  }

  /**
   * FAST cancels an event whose handler does not return `true`, so every
   * keydown handler here returns it and calls `preventDefault` itself where it
   * means to. Forgetting is not a subtle bug: it stops the reader typing.
   */
  onPageKeydown(event: KeyboardEvent): boolean {
    if (event.key === 'Escape') {
      this.showPage();
      this.scrollEl?.focus();
    }
    return true;
  }

  onZoomEntered(event: Event): void {
    const percent = parseZoomPercent((event.target as HTMLInputElement).value);
    if (percent === null) this.showZoom();
    else this.setZoom(percent / 100);
  }

  onZoomKeydown(event: KeyboardEvent): boolean {
    if (event.key === 'Escape') {
      this.showZoom();
      this.scrollEl?.focus();
    }
    return true;
  }

  onViewerKeydown(event: KeyboardEvent): boolean {
    if (event.ctrlKey || event.metaKey || event.altKey) return true;
    if (event.key === 'PageDown') this.goToPage(this.page + 1);
    else if (event.key === 'PageUp') this.goToPage(this.page - 1);
    else if (event.key === 'Home') this.goToPage(1);
    else if (event.key === 'End') this.goToPage(this.pageCount);
    else if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
      // A page zoomed past the width of the tab has somewhere to go sideways,
      // and taking that away would leave no way to read its right-hand edge
      // without a mouse. Only when there is nothing to scroll do these turn
      // the page — which is the rule pdf.js's own viewer follows.
      if (this.scrollsSideways()) return true;
      this.goToPage(this.page + (event.key === 'ArrowRight' ? 1 : -1));
    } else return true;
    event.preventDefault();
    return true;
  }

  /** Whether the column is wider than the tab, so there is width to scroll. */
  private scrollsSideways(): boolean {
    const el = this.scrollEl as HTMLElement | undefined;
    return el !== undefined && el.scrollWidth > el.clientWidth + 1;
  }

  toggleInvert(): void {
    this.inverted = !this.inverted;
    this.reportPlace();
  }

  // ── The toolbar's tooltips ─────────────────────────────────────────────────

  /**
   * The toolbar explains itself.
   *
   * A row of glyphs is unreadable until something names them, and `title` is
   * the wrong instrument for it: a second of hover before anything appears, an
   * OS tooltip that knows nothing of the editor's theme, and nowhere to put the
   * shortcut that does the same thing. So the tip is ours.
   *
   * Wired once on the toolbar rather than per control: `pointerover` and
   * `focusin` bubble, so one pair of handlers covers everything in it — and a
   * control added later needs nothing but its `data-tip`. `pointerover` also
   * fires for the toolbar's own background, which is what dismisses a tip when
   * the pointer slides off a button into the gap beside it.
   */
  onTipOver(event: Event): boolean {
    this.armTip(tipTarget(event.target), false);
    return true;
  }

  /** The same, for a reader on the keyboard: no hover, so no wait either. */
  onTipFocus(event: Event): boolean {
    const target = tipTarget(event.target);
    // Buttons only. A tip hanging under the box the reader is typing in is in
    // the way of the thing they are typing at.
    this.armTip(target?.classList.contains('btn') === true ? target : null, true);
    return true;
  }

  onTipLeave(): boolean {
    this.hideTip();
    return true;
  }

  private armTip(target: HTMLElement | null, now: boolean): void {
    if (target === this.tipAnchor) return;
    // Once one tip is up the next follows the pointer without a wait, which is
    // what makes a row of unlabelled buttons readable in a single pass.
    const following = this.tip !== null;
    this.hideTip();
    if (!target) return;
    this.tipAnchor = target;
    if (now || following) this.showTip(target);
    else this.tipTimer = setTimeout(() => this.showTip(target), TIP_MS);
  }

  private showTip(target: HTMLElement): void {
    this.tipTimer = undefined;
    if (this.tipAnchor !== target) return;
    this.tip = { text: target.dataset.tip ?? '', keys: target.dataset.keys ?? '' };
    // Hidden until it has been measured: a tip is placed from its own width,
    // and its width is not known until this text is in it.
    if (this.tipEl) this.tipEl.style.visibility = 'hidden';
    void Updates.next().then(() => this.placeTip(target));
  }

  /**
   * Centre the tip under the control, clamped to the viewer's own edges — the
   * tips at either end of the toolbar would otherwise hang off the side of the
   * tab, where nothing can be read.
   */
  private placeTip(target: HTMLElement): void {
    const el = this.tipEl;
    if (!el || this.tipAnchor !== target || !target.isConnected) return;
    const host = this.getBoundingClientRect();
    const box = target.getBoundingClientRect();
    const left = box.left - host.left + (box.width - el.offsetWidth) / 2;
    el.style.left = `${Math.round(clampTip(left, host.width - el.offsetWidth))}px`;
    el.style.top = `${Math.round(box.bottom - host.top + TIP_GAP)}px`;
    el.style.visibility = 'visible';
  }

  private hideTip(): void {
    if (this.tipTimer) clearTimeout(this.tipTimer);
    this.tipTimer = undefined;
    this.tipAnchor = null;
    this.tip = null;
    if (this.tipEl) this.tipEl.style.visibility = 'hidden';
  }

  // ── The outline ────────────────────────────────────────────────────────────

  toggleOutline(): void {
    this.outlineVisible = !this.outlineVisible;
    this.reportPlace();
    // The scroller just changed width, so a fit is now a different number.
    void Updates.next().then(() => this.applyZoom());
  }

  isCollapsed(row: OutlineRow): boolean {
    return this.outline.isCollapsed(row);
  }

  toggleOutlineRow(row: OutlineRow, event: Event): void {
    // Without this the click reaches the row behind the twisty, and expanding
    // a chapter would also navigate to it.
    event.stopPropagation();
    this.outline.toggle(row);
  }

  onOutlineKeydown(row: OutlineRow, event: KeyboardEvent): boolean {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      this.followOutline(row);
    } else if (event.key === 'ArrowLeft' && row.hasChildren) {
      this.outline.setCollapsed(row, true);
    } else if (event.key === 'ArrowRight' && row.hasChildren) {
      this.outline.setCollapsed(row, false);
    }
    return true;
  }

  followOutline(row: OutlineRow): void {
    if (row.url) {
      this.host.post({ type: 'openLink', href: row.url });
      return;
    }
    void this.jumpTo(row.dest ?? null);
  }

  resetOutlineWidth(): void {
    this.outlineWidth = this.settings.outlineWidth;
    void Updates.next().then(() => this.applyZoom());
  }

  /**
   * Drag the sidebar's edge. Pointer capture rather than window listeners: the
   * drag has to survive the pointer crossing the page column, which is a
   * scroller and would otherwise swallow it.
   */
  onResizeStart(event: PointerEvent): void {
    const handle = event.currentTarget as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    this.resizePointer = {
      id: event.pointerId,
      startX: event.clientX,
      startWidth: this.outlineWidth,
    };
    const move = (moved: PointerEvent): void => {
      if (this.resizePointer?.id !== moved.pointerId) return;
      const width = this.resizePointer.startWidth + (moved.clientX - this.resizePointer.startX);
      this.outlineWidth = Math.min(720, Math.max(140, Math.round(width)));
    };
    const end = (): void => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', end);
      handle.removeEventListener('pointercancel', end);
      this.resizePointer = null;
      this.applyZoom();
      this.reportPlace();
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', end);
    handle.addEventListener('pointercancel', end);
    event.preventDefault();
  }

  // ── Links and destinations ─────────────────────────────────────────────────

  private async followLink(target: LinkTarget): Promise<void> {
    if (target.kind === 'url') {
      this.host.post({ type: 'openLink', href: target.href });
      return;
    }
    await this.jumpTo(target.dest);
  }

  /** Resolve a destination and scroll to it. */
  private async jumpTo(dest: string | unknown[] | null): Promise<void> {
    const doc = this.doc;
    if (!doc) return;
    const at = await resolveDestination(doc, dest);
    // Recorded only once the destination is known to be somewhere: a link into
    // a page that is not there leaves the reader where they are, and a back
    // step that goes nowhere is worse than no back step.
    if (!at) return;
    this.recordJump();
    this.page = at.page;
    this.showPage();
    this.column?.revealPoint(at.page, at.point);
    this.reportPlace();
  }

  // ── Find ───────────────────────────────────────────────────────────────────

  focusFind(): void {
    this.findInput?.focus();
    this.findInput?.select();
  }

  onFindInput(event: Event): void {
    this.search.type((event.target as HTMLInputElement).value);
  }

  onFindKeydown(event: KeyboardEvent): boolean {
    if (event.key === 'Escape') {
      this.search.clear();
      this.scrollEl?.focus();
      return true;
    }
    if (event.key !== 'Enter') return true;
    event.preventDefault();
    // Enter before the debounce elapsed: run the search now rather than
    // stepping through matches that have not been found yet.
    if (this.search.pending) void this.search.run();
    else void this.search.step(event.shiftKey ? -1 : 1);
    return true;
  }

  /** Put a found match a third of the way down the viewport. */
  private revealRange(range: Range): void {
    if (!this.scrollEl) return;
    const rect = range.getBoundingClientRect();
    const view = this.scrollEl.getBoundingClientRect();
    this.scrollEl.scrollTop += rect.top - view.top - this.scrollEl.clientHeight / 3;
  }

  // ── Export ─────────────────────────────────────────────────────────────────

  private async exportPage(): Promise<void> {
    const column = this.column;
    if (!column) return;
    this.say('Rendering the page…');
    try {
      const data = await column.renderPng(this.page, EXPORT_ZOOM);
      if (data) this.host.post({ type: 'pagePng', page: this.page, data });
      this.say('');
    } catch (error) {
      this.say('');
      this.host.post({
        type: 'error',
        message: describe(error),
        context: 'exportPagePng',
      });
    }
  }

  // ── Odds and ends ──────────────────────────────────────────────────────────

  private say(message: string): void {
    this.notice = message;
    if (this.noticeTimer) clearTimeout(this.noticeTimer);
    if (message === '') return;
    this.noticeTimer = setTimeout(() => {
      this.noticeTimer = undefined;
      this.notice = '';
    }, NOTICE_MS);
  }

  /**
   * Recompute a fit when the tab is resized — and notice when it was not
   * resized at all but taken away and given back.
   *
   * VSCode keeps this webview alive in the background and lays it out at
   * nothing while it is there, so a hidden tab arrives here as a 0×0
   * observation. Both of the things this normally does are wrong to do then,
   * and the observation that follows — the same size the tab had before it
   * went away — would otherwise look like nothing had happened at all.
   */
  private observeResize(): void {
    if (typeof ResizeObserver === 'undefined') return;
    this.resizeObserver?.disconnect();
    this.resizeAttached = this.scrollEl;
    this.resizeObserver = new ResizeObserver(() => {
      const view = this.column?.viewSize;
      if (!view || view.w === 0 || view.h === 0) {
        this.wasOffScreen = true;
        return;
      }
      if (this.wasOffScreen) {
        this.revalidate();
        return;
      }
      this.applyZoom();
      this.column?.relayout();
    });
    this.resizeObserver.observe(this.scrollEl);
  }

  /**
   * Take the column over after the tab was away.
   *
   * A hidden webview is given no animation frames, and pdf.js continues a
   * display render on one — so a page that started drawing as the tab went
   * into the background is still part-drawn, still wearing its placeholder,
   * and nothing about the scroller changed while it was gone for the usual
   * guards to catch. This is the cue to lay out and draw again regardless.
   */
  private revalidate(): void {
    const column = this.column;
    if (!column) return;
    const view = column.viewSize;
    // The host's `visible` can land before the tab has been laid out again.
    // Leaving the flag set is what makes the resize observation that follows
    // come back here rather than treat the tab as one that never went away.
    if (view.w === 0 || view.h === 0) return;
    this.wasOffScreen = false;
    this.applyZoom();
    column.refresh();
  }
}

/**
 * A wheel event's vertical delta in pixels, whatever unit it arrived in.
 *
 * `deltaMode` is not decoration: a mouse wheel in Firefox reports lines and a
 * page-mode device reports screens, and reading either as pixels turns one
 * notch into no zoom at all.
 */
function wheelPixels(event: WheelEvent): number {
  if (event.deltaMode === 1) return event.deltaY * WHEEL_LINE_PX;
  if (event.deltaMode === 2) return event.deltaY * WHEEL_PAGE_PX;
  return event.deltaY;
}

/** The control a pointer or focus event landed on, if it has anything to say. */
function tipTarget(target: EventTarget | null): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>('[data-tip]') : null;
}

/** Keep a tip's left edge inside the viewer, with a margin either side. */
function clampTip(left: number, max: number): number {
  return Math.min(Math.max(left, TIP_GAP), Math.max(TIP_GAP, max - TIP_GAP));
}

function prefersDark(): boolean {
  return (
    document.body.classList.contains('vscode-dark') ||
    document.body.classList.contains('vscode-high-contrast')
  );
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

declare global {
  interface HTMLElementTagNameMap {
    'pdf-viewer': PdfViewer;
  }
}
