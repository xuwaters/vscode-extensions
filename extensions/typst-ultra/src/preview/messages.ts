/**
 * The host ⇄ webview protocol.
 *
 * Imported by both sides, so a change to one is a type error in the other.
 *
 * Two rules keep it honest:
 *
 * * **`seq` is monotonic per document.** The webview ignores any `metrics` or
 *   `pages` message whose `seq` is not greater than the last one it applied, so
 *   an out-of-order or superseded compile cannot corrupt the view.
 * * **Every `WebviewToHost` message is shape-validated host-side** by a
 *   hand-written guard per variant. No `any` dispatch. A webview is a hostile
 *   input boundary even when we wrote the code on the other side of it.
 */

/**
 * How the page column is fitted to the panel.
 *
 * A *mode*, not an action: `width` and `page` stay switched on and are
 * re-resolved to a new zoom whenever the panel changes size, until something
 * that names a zoom outright — the buttons, the box, Ctrl+0 — turns them off by
 * switching to `actual`.
 */
export type FitMode = 'width' | 'page' | 'actual';

/** The parts of the configuration the webview needs. */
export interface PreviewSettings {
  scrollSync: 'both' | 'editorToPreview' | 'previewToEditor' | 'off';
  cursorIndicator: boolean;
  invertColors: 'never' | 'always' | 'auto';
  background: 'editor' | 'white' | 'gray';
  /**
   * `png` is a low-memory mode that loses zoom fidelity and find-in-preview;
   * `auto` switches per page above roughly 1 MB of SVG.
   */
  renderMode: 'svg' | 'png' | 'auto';
}

/** One page's placeholder geometry and identity. */
export interface PageMetric {
  index: number;
  widthPt: number;
  heightPt: number;
  hash: string;
}

/** How a rendered page arrived. */
export type PageFormat = 'svg' | 'png';

/** What the server says to do with one page. */
export type PagePatch =
  | {
      op: 'replace';
      index: number;
      hash: string;
      /** `svg` markup, or base64 PNG. */
      format: PageFormat;
      content: string;
    }
  | { op: 'unchanged'; index: number }
  | { op: 'removed'; index: number };

/**
 * How the reader last had the preview set up.
 *
 * Kept twice over: by the webview itself, through `setState`, which is what
 * survives a reload of the same panel; and by the host in workspace storage, so
 * a *new* surface — the tab a mode switch just opened, a panel in the next
 * window — starts the way the last one was left rather than resetting the fit
 * every time the preview moves.
 */
export interface PreviewPlace {
  zoom: number;
  fit: FitMode;
  inverted: boolean;
}

/** Host → webview. */
export type HostToWebview =
  | { type: 'init'; settings: PreviewSettings; restore?: PreviewPlace }
  | { type: 'metrics'; seq: number; uri: string; pages: PageMetric[] }
  | { type: 'pages'; seq: number; patches: PagePatch[] }
  | { type: 'cursor'; page: number; xPt: number; yPt: number }
  | { type: 'status'; state: 'compiling' | 'ok' | 'error'; message?: string }
  | { type: 'settings'; settings: PreviewSettings }
  /**
   * The tab has become the active one. VSCode focuses the page itself but
   * nothing in it, and the page keys act on whatever holds the focus — so the
   * webview is told to put it on the page column.
   */
  | { type: 'focus' }
  | { type: 'goToPage'; page: number };

/** Webview → host. */
export type WebviewToHost =
  | { type: 'ready' }
  | {
      type: 'viewport';
      first: number;
      last: number;
      known: Record<number, string>;
      /** Current zoom, so a raster page is rasterized at the size it is shown. */
      zoom: number;
    }
  | { type: 'click'; page: number; xPt: number; yPt: number }
  | { type: 'scrolled'; page: number; yPt: number }
  | { type: 'openLink'; href: string }
  | { type: 'state'; zoom: number; fit: FitMode; inverted: boolean }
  /** The toolbar's Export button: the same command as the title-bar icon. */
  | { type: 'export' }
  /** The toolbar's Edit button: hand the reader back to the source. */
  | { type: 'openSource' }
  | { type: 'error'; message: string; context: string };

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isPageIndex(value: unknown): value is number {
  return isNumber(value) && Number.isInteger(value) && value >= 0 && value < 100_000;
}

/**
 * Validate a message from the webview.
 *
 * One guard per variant, hand-written. Returns null for anything that does not
 * match exactly, which the caller logs and drops.
 */
export function parseWebviewMessage(value: unknown): WebviewToHost | null {
  if (!isObject(value) || typeof value.type !== 'string') return null;

  switch (value.type) {
    case 'ready':
      return { type: 'ready' };

    case 'viewport': {
      if (!isPageIndex(value.first) || !isPageIndex(value.last)) return null;
      if (value.last < value.first) return null;
      if (!isObject(value.known)) return null;

      const known: Record<number, string> = {};
      for (const [key, hash] of Object.entries(value.known)) {
        const index = Number(key);
        if (!isPageIndex(index) || typeof hash !== 'string') return null;
        // Hashes are 16 hex digits; anything else is not something we produced.
        if (!/^[0-9a-f]{16}$/.test(hash)) return null;
        known[index] = hash;
      }
      if (!isNumber(value.zoom) || value.zoom <= 0 || value.zoom > 20) return null;
      return {
        type: 'viewport',
        first: value.first,
        last: value.last,
        known,
        zoom: value.zoom,
      };
    }

    case 'click': {
      if (!isPageIndex(value.page)) return null;
      if (!isNumber(value.xPt) || !isNumber(value.yPt)) return null;
      return { type: 'click', page: value.page, xPt: value.xPt, yPt: value.yPt };
    }

    case 'scrolled': {
      if (!isPageIndex(value.page) || !isNumber(value.yPt)) return null;
      return { type: 'scrolled', page: value.page, yPt: value.yPt };
    }

    case 'openLink': {
      if (typeof value.href !== 'string' || value.href.length > 4096) return null;
      return { type: 'openLink', href: value.href };
    }

    case 'state': {
      const place = parsePreviewPlace(value);
      return place ? { type: 'state', ...place } : null;
    }

    case 'export':
      return { type: 'export' };

    case 'openSource':
      return { type: 'openSource' };

    case 'error': {
      if (typeof value.message !== 'string' || typeof value.context !== 'string') {
        return null;
      }
      return {
        type: 'error',
        message: value.message.slice(0, 2000),
        context: value.context.slice(0, 200),
      };
    }

    default:
      return null;
  }
}

/**
 * Validate a remembered setup.
 *
 * Used twice: on the way in from the webview, where it is untrusted input like
 * everything else, and on the way back *out* of workspace storage, which is a
 * file on disk that an older version of this extension — or a hand edit — may
 * have left in a shape this one does not accept.
 */
export function parsePreviewPlace(value: unknown): PreviewPlace | null {
  if (!isObject(value)) return null;
  if (!isNumber(value.zoom) || value.zoom <= 0 || value.zoom > 20) return null;
  if (value.fit !== 'width' && value.fit !== 'page' && value.fit !== 'actual') {
    return null;
  }
  if (typeof value.inverted !== 'boolean') return null;
  return { zoom: value.zoom, fit: value.fit, inverted: value.inverted };
}

/** Schemes the host will hand to `env.openExternal`. */
export const ALLOWED_LINK_SCHEMES = ['https:', 'http:', 'mailto:'];

/**
 * Whether a link from the preview may be opened externally.
 *
 * The webview never navigates; it asks, and this decides.
 */
export function isAllowedLink(href: string): boolean {
  try {
    return ALLOWED_LINK_SCHEMES.includes(new URL(href).protocol);
  } catch {
    return false;
  }
}
