import { FASTElement, css, customElement, observable } from '@microsoft/fast-element';
import type {
  FitMode,
  HostToWebview,
  PreviewPlace,
  PreviewSettings,
  WebviewToHost,
} from '../../src/preview/messages.js';
import { PX_PER_PT, clampZoom, fitZoom, stepZoom, zoomBucket } from '../model/layout.js';
import { formatZoomPercent, parseZoomPercent } from '../model/zoom.js';
import { PageColumn } from '../render/pageColumn.js';
import sheet from './styles.css';
import { template } from './template.js';

/** How long the cursor indicator stays on the page before it has faded out. */
const CURSOR_MS = 1600;

/** How long a scroll settles before the editor beside us is told about it. */
const SCROLL_MS = 120;

/** How long a change settles before the host is asked to remember it. */
const SAVE_MS = 200;

/** What the element needs from the extension host. */
export interface PreviewHost {
  post(message: WebviewToHost): void;
  /** Remember how the preview is set up, for the next surface that opens. */
  save(place: PreviewPlace): void;
}

const styles = css`
  ${sheet}
`;

/**
 * The tag, named once.
 *
 * `@customElement` in fast-element 3 defines the element *asynchronously* —
 * `compose` resolves a promise before `customElements.define` is ever called —
 * so the bootstrap has to wait on this name rather than construct the class the
 * moment the module has evaluated.
 */
export const TYPST_PREVIEW_TAG = 'typst-preview';

/**
 * `<typst-preview>` — the whole preview.
 *
 * The split is the one `pdf-ultra` uses and for the same reason: the *chrome*
 * is reactive and lives in the template, and the *pages* are not. `PageColumn`
 * owns the column imperatively — most of its boxes are empty placeholders at
 * any moment — and this element owns everything the reader can see the state of.
 *
 * **Light DOM, deliberately.** `shadowOptions: null` is not a style preference:
 * the panel is created with `enableFindWidget`, and VSCode implements that with
 * `window.find` over the webview's document. `window.find` does not cross a
 * shadow boundary, so a shadow root would silently take find-in-preview away —
 * and find works here for free, because the compiler's SVG carries real `<text>`
 * runs. The stylesheet therefore lands on the document rather than in a shadow
 * root, which is fine: this page is nothing but this element.
 */
@customElement({ name: TYPST_PREVIEW_TAG, template, styles, shadowOptions: null })
export class TypstPreview extends FASTElement {
  /** Set by the bootstrap before the element is attached. */
  host!: PreviewHost;

  @observable settings: PreviewSettings = {
    scrollSync: 'both',
    cursorIndicator: true,
    invertColors: 'never',
    background: 'editor',
    renderMode: 'svg',
  };

  @observable fit: FitMode = 'width';
  @observable zoom = 1;
  @observable zoomField = '100%';
  @observable inverted = false;
  @observable pageCount = 0;
  @observable pageField = '1';
  @observable status: { state: 'compiling' | 'ok' | 'error'; message?: string } = {
    state: 'ok',
  };

  /** Bound by the template. */
  scrollEl!: HTMLElement;
  zoomInput!: HTMLInputElement;
  pageInput!: HTMLInputElement;

  private column: PageColumn | null = null;
  /** Highest `seq` applied. Anything older is a superseded compile. */
  private appliedSeq = 0;
  /** The document on screen, so a change of subject can be told from an edit. */
  private shownUri: string | undefined;
  /** Whether the reader's own inversion choice has overridden the setting. */
  private invertedByReader = false;
  /** Whether a remembered setup has already been adopted. */
  private restored = false;
  /** Whether the host has told us the configuration yet. */
  private settingsSeen = false;

  private scrollTimer: ReturnType<typeof setTimeout> | undefined;
  private saveTimer: ReturnType<typeof setTimeout> | undefined;

  private resizeObserver: ResizeObserver | null = null;

  /**
   * Zoom from the keyboard.
   *
   * On `window` rather than on the element: the reader may have the focus in
   * the toolbar's own boxes, and Ctrl+= there means the same thing it means
   * over the page.
   */
  private readonly onKeydown = (event: KeyboardEvent): void => {
    if (!event.ctrlKey && !event.metaKey) return;

    if (event.key === '+' || event.key === '=') this.zoomBy(1);
    else if (event.key === '-') this.zoomBy(-1);
    else if (event.key === '0') this.applyFit('actual');
    else return;
    event.preventDefault();
  };

  override connectedCallback(): void {
    super.connectedCallback();
    // The template has rendered by now — it is not behind a `when`, so the refs
    // are already bound and the scroller exists to be taken over.
    this.column = new PageColumn(this.scrollEl, {
      onViewport: (first, last, known) =>
        this.host.post({ type: 'viewport', first, last, known, zoom: this.zoom }),
      onClick: (page, xPt, yPt) => this.host.post({ type: 'click', page, xPt, yPt }),
    });
    this.applyZoom();
    this.observeResize();
    window.addEventListener('keydown', this.onKeydown);
    // The arrows are bound on the column, so they only reach it while it has the
    // focus. Nothing in a fresh panel has it, and asking the reader to click the
    // page before they can turn it is a keyboard reader's worst tab — so the
    // column takes it up front. This moves nothing outside the webview: focus
    // inside a document the editor has not focused stays latent until it is.
    this.scrollEl.focus({ preventScroll: true });
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    window.removeEventListener('keydown', this.onKeydown);
    if (this.scrollTimer) clearTimeout(this.scrollTimer);
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    this.column?.destroy();
    this.column = null;
  }

  /** What the page column wears: the reader's inversion, and a failed compile. */
  get columnClass(): string {
    return [
      this.inverted ? 'inverted' : '',
      this.status.state === 'error' ? 'has-error' : '',
    ]
      .filter(Boolean)
      .join(' ');
  }

  get statusText(): string {
    if (this.status.state === 'compiling') return 'Compiling…';
    return (
      this.status.message ?? 'The document has errors — showing the last good version'
    );
  }

  // ── The host protocol ──────────────────────────────────────────────────────

  /** Everything the extension host says arrives here. */
  handle(message: HostToWebview): void {
    switch (message.type) {
      case 'init':
        // The host's memory is the fallback, not the authority: a webview that
        // still holds its own state was restored from it before this arrived,
        // and that copy is the one this panel was last left in.
        if (message.restore) this.restore(message.restore);
        this.applySettings(message.settings);
        break;

      case 'settings':
        this.applySettings(message.settings);
        break;

      case 'metrics':
        // A message from a superseded compile can never overwrite a newer one.
        if (message.seq <= this.appliedSeq) return;
        this.appliedSeq = message.seq;
        // The panel follows the active editor, so a new URI here means the
        // reader opened a different document — not that this one changed.
        if (this.shownUri !== undefined && this.shownUri !== message.uri) {
          this.column?.reset();
        }
        this.shownUri = message.uri;
        this.column?.setMetrics(message.pages);
        this.pageCount = this.column?.length ?? 0;
        // The page sizes may have changed with the document, so a fit that is
        // switched on means a different zoom than it did a moment ago.
        this.applyZoom();
        break;

      case 'pages':
        if (message.seq <= this.appliedSeq) return;
        this.appliedSeq = message.seq;
        this.column?.applyPatches(message.patches);
        break;

      case 'cursor':
        this.showCursor(message.page, message.yPt);
        break;

      case 'status':
        this.status = { state: message.state, message: message.message };
        break;

      case 'goToPage':
        // A negative page is the invert command in disguise: the host has no
        // other way to reach a toggle that lives only in the webview.
        if (message.page >= 0) this.goToPage(message.page);
        else this.toggleInvert();
        break;
    }
  }

  // ── Settings and place ─────────────────────────────────────────────────────

  private applySettings(next: PreviewSettings): void {
    const before = this.settings;
    // The defaults this element was constructed with are a placeholder, not a
    // reading of the configuration, so the first push is never a *change* —
    // otherwise it would look like one and overrule a restored setup.
    const changed = this.settingsSeen && before.invertColors !== next.invertColors;
    this.settingsSeen = true;
    this.settings = next;

    // The toolbar's toggle is the reader's, and a settings push must not undo
    // it — only a change to the setting overrides what they chose.
    if (!this.invertedByReader || changed) {
      this.inverted =
        next.invertColors === 'always' ||
        (next.invertColors === 'auto' && prefersDark());
      this.invertedByReader = false;
    }

    // Switching into or out of a raster mode invalidates what the column holds,
    // because the same page hash now has to arrive in a different format.
    if ((before.renderMode !== 'svg') !== (next.renderMode !== 'svg')) {
      this.column?.forget();
    }
  }

  /**
   * Adopt a remembered setup.
   *
   * First one wins: the webview's own `setState` copy arrives before the host's
   * fallback and describes this very panel, so `override` is what the bootstrap
   * passes and the `init` message does not.
   */
  restore(place: PreviewPlace, options: { override?: boolean } = {}): void {
    if (this.restored && options.override !== true) return;
    this.restored = true;
    this.fit = place.fit;
    this.zoom = clampZoom(place.zoom);
    this.inverted = place.inverted;
    this.invertedByReader = true;
    this.applyZoom();
  }

  private currentPlace(): PreviewPlace {
    return { zoom: this.zoom, fit: this.fit, inverted: this.inverted };
  }

  /** Tell the host how the preview is set up, at most a few times a second. */
  private savePlace(): void {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => {
      this.saveTimer = undefined;
      this.host.save(this.currentPlace());
    }, SAVE_MS);
  }

  // ── Zoom and fit ───────────────────────────────────────────────────────────

  /**
   * Resolve the fit that is in force into a scale and hand it to the column.
   *
   * A tab with no box is left alone: a fit measured against a zero-width
   * scroller resolves to the bottom of the zoom range, and writing that into
   * the layout is how a document comes back from the background at 10%.
   */
  private applyZoom(): void {
    const column = this.column;
    if (!column) return;

    if (this.fit !== 'actual') {
      const page = column.baseGeom;
      const view = column.viewSize;
      // Nothing to measure, or nothing to measure against: keep the fit switched
      // on and resolve it when there is.
      if (!page || view.w === 0 || view.h === 0) return;
      this.zoom = fitZoom(this.fit, view, page);
    }

    const before = column.scale;
    column.setZoom(this.zoom);
    this.zoom = column.scale;
    this.showZoom();

    // A raster page is drawn at a fixed resolution, so a real zoom step needs it
    // drawn again or it goes soft. Vector pages just scale.
    if (
      this.settings.renderMode !== 'svg' &&
      zoomBucket(before) !== zoomBucket(column.scale)
    ) {
      column.forget();
    }
  }

  /**
   * Switch a fit on, or go back to actual size.
   *
   * A fit is a mode: it stays on, and every resize of the panel re-resolves it,
   * until the reader names a zoom themselves.
   */
  applyFit(fit: FitMode): void {
    this.fit = fit;
    if (fit === 'actual') this.zoom = 1;
    this.applyZoom();
    this.savePlace();
  }

  /** One step of the zoom buttons, which switches any fit off. */
  zoomBy(direction: 1 | -1): void {
    this.setZoom(stepZoom(this.zoom, direction));
  }

  setZoom(zoom: number): void {
    this.fit = 'actual';
    this.zoom = clampZoom(zoom);
    this.applyZoom();
    this.savePlace();
  }

  toggleInvert(): void {
    this.inverted = !this.inverted;
    this.invertedByReader = true;
    this.savePlace();
  }

  // ── The toolbar's boxes ────────────────────────────────────────────────────

  /**
   * Write a value back into its box.
   *
   * Both of these set the element's value as well as the observable. A binding
   * only pushes when the value it reads *changes*, and the case that matters
   * most is exactly the one where it does not: the reader typed something
   * unreadable into the box, the correction is the number that was already
   * there, and without this the box keeps showing the nonsense.
   */
  showZoom(): void {
    this.zoomField = formatZoomPercent(this.zoom);
    if (this.zoomInput) this.zoomInput.value = this.zoomField;
  }

  showPage(): void {
    if (this.pageInput) this.pageInput.value = this.pageField;
  }

  onZoomEntered(event: Event): void {
    const percent = parseZoomPercent((event.target as HTMLInputElement).value);
    if (percent === null) this.showZoom();
    else this.setZoom(percent / 100);
  }

  /**
   * FAST cancels an event whose handler does not return `true`, so every
   * keydown handler here returns it and calls `preventDefault` itself where it
   * means to. Forgetting is not a subtle bug: it stops the reader typing.
   */
  onZoomKeydown(event: KeyboardEvent): boolean {
    if (event.key === 'Escape') {
      this.showZoom();
      this.scrollEl?.focus();
    }
    return true;
  }

  /**
   * A page number typed into the box.
   *
   * Out of range is not a typo: 0 and 900 in a 300-page document both name an
   * end of it, so they go there rather than being thrown away. Only text that is
   * not a number at all leaves the reader where they are, with the box put back
   * to the page they are on.
   */
  onPageEntered(event: Event): void {
    const text = (event.target as HTMLInputElement).value.trim();
    const value = Number(text);
    if (text !== '' && Number.isFinite(value)) this.goToPage(Math.round(value) - 1);
    else this.showPage();
  }

  onPageKeydown(event: KeyboardEvent): boolean {
    if (event.key === 'Escape') {
      this.showPage();
      this.scrollEl?.focus();
    }
    return true;
  }

  // ── Navigation ─────────────────────────────────────────────────────────────

  /**
   * Turn the page from the keyboard — the same set `pdf-ultra` answers to, so
   * that the two viewers in this repo are read the same way.
   *
   * Bound on the column rather than on `window` like the zoom keys are: an arrow
   * in the page box or the zoom box moves the caret through the number being
   * typed, and taking that away would stop the reader editing it.
   */
  onColumnKeydown(event: KeyboardEvent): boolean {
    if (event.ctrlKey || event.metaKey || event.altKey) return true;

    if (event.key === 'PageDown') this.goToPage(this.shownPage() + 1);
    else if (event.key === 'PageUp') this.goToPage(this.shownPage() - 1);
    else if (event.key === 'Home') this.goToPage(0);
    else if (event.key === 'End') this.goToPage(this.pageCount - 1);
    else if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
      // A page zoomed past the width of the tab has somewhere to go sideways,
      // and taking that away would leave no way to read its right-hand edge
      // without a mouse. Only when there is nothing to scroll do these turn the
      // page — which is the rule pdf.js's own viewer follows.
      if (this.scrollsSideways()) return true;
      this.goToPage(this.shownPage() + (event.key === 'ArrowRight' ? 1 : -1));
    } else return true;

    event.preventDefault();
    return true;
  }

  /** Whether the column is wider than the tab, so there is width to scroll. */
  private scrollsSideways(): boolean {
    const el = this.scrollEl as HTMLElement | undefined;
    return el !== undefined && el.scrollWidth > el.clientWidth + 1;
  }

  /**
   * The page the reader is on, by index, as the toolbar has it.
   *
   * Read off the box rather than measured off the scroller, because the box is
   * what the reader can see: `goToPage` writes it and scrolling follows it, so
   * turning the page from it is what makes "3 / 12" and one press of → agree
   * that the next page is 4.
   */
  private shownPage(): number {
    const value = Number(this.pageField);
    return Number.isFinite(value) ? Math.round(value) - 1 : 0;
  }

  /** Scroll to a page, by index. */
  goToPage(index: number): void {
    const last = Math.max(0, (this.column?.length ?? 1) - 1);
    const target = Math.min(Math.max(0, index), last);
    this.column?.goToPage(target);
    this.pageField = String(target + 1);
    this.showPage();
  }

  openSource(): void {
    this.host.post({ type: 'openSource' });
  }

  exportDocument(): void {
    this.host.post({ type: 'export' });
  }

  onScroll(): boolean {
    this.updateCurrentPage();

    const sync = this.settings.scrollSync;
    if (sync !== 'both' && sync !== 'previewToEditor') return true;

    if (this.scrollTimer) clearTimeout(this.scrollTimer);
    this.scrollTimer = setTimeout(() => {
      this.scrollTimer = undefined;
      const center = this.column?.centerPage();
      if (center) this.host.post({ type: 'scrolled', page: center.page, yPt: center.yPt });
    }, SCROLL_MS);
    return true;
  }

  /**
   * Links inside the SVG are handled here rather than by navigating: the
   * webview never leaves the page, and an internal `#` target is the compiler's
   * own business.
   */
  onColumnClick(event: Event): boolean {
    const target = event.target;
    if (!(target instanceof Element)) return true;
    const href = target.closest('a')?.getAttribute('href');
    if (!href || href.startsWith('#')) return true;

    event.preventDefault();
    this.host.post({ type: 'openLink', href });
    return true;
  }

  private updateCurrentPage(): void {
    // Not while the reader is typing in it: following the scroll would rewrite
    // the number half-way through the one they are entering.
    if (document.activeElement === this.pageInput) return;

    const center = this.column?.centerPage();
    if (!center) return;
    this.pageField = String(center.page + 1);
    this.showPage();
  }

  private showCursor(page: number, yPt: number): void {
    const element = this.column?.reveal(page, yPt);
    if (!element || !this.settings.cursorIndicator) return;

    const marker = document.createElement('div');
    marker.className = 'cursor-indicator';
    marker.style.top = `${yPt * PX_PER_PT * this.zoom}px`;
    element.append(marker);

    // Fades out via CSS animation; remove it once it has.
    setTimeout(() => marker.remove(), CURSOR_MS);
  }

  // ── Resize ─────────────────────────────────────────────────────────────────

  /**
   * Re-resolve the fit when the panel is resized — and notice when it was not
   * resized at all but taken away and given back.
   *
   * This is what makes a fit a mode rather than a one-shot: dragging the split
   * wider with Fit Width on keeps the page filling the width, the way every
   * other document viewer behaves.
   *
   * VSCode keeps this webview alive in the background and lays it out at
   * nothing while it is there, so a hidden tab arrives here as a 0×0
   * observation. Resolving a fit against that would land at the bottom of the
   * range, so it is skipped — and the observation that follows, the same size
   * the tab had before it went away, is the cue to lay out again.
   */
  private observeResize(): void {
    if (typeof ResizeObserver === 'undefined') return;
    this.resizeObserver?.disconnect();
    this.resizeObserver = new ResizeObserver(() => this.revalidate());
    this.resizeObserver.observe(this.scrollEl);
  }

  /** Lay out again: the panel changed size, or came back from the background. */
  revalidate(): void {
    const view = this.column?.viewSize;
    if (!view || view.w === 0 || view.h === 0) return;
    this.applyZoom();
    // Even at an unchanged zoom the panel now shows a different band of pages.
    this.column?.reportViewport();
  }
}

function prefersDark(): boolean {
  return (
    document.body.classList.contains('vscode-dark') ||
    document.body.classList.contains('vscode-high-contrast')
  );
}

declare global {
  interface HTMLElementTagNameMap {
    'typst-preview': TypstPreview;
  }
}
