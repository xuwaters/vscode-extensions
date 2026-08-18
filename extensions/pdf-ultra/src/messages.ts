/**
 * The host ⇄ webview protocol.
 *
 * Imported by both sides, so a change to one is a type error in the other.
 *
 * Every `WebviewToHost` message is shape-validated host-side by a hand-written
 * guard per variant — no `any` dispatch. A webview is a hostile input boundary
 * even when we wrote the code on the other side of it, and this one renders
 * documents that can come from anywhere.
 */

/** How the page column is fitted to the tab. A fit recomputes on resize. */
export type FitMode = 'fit-width' | 'fit-page' | 'actual';

/** Quarter turns clockwise, applied on top of each page's own `/Rotate`. */
export type Rotation = 0 | 90 | 180 | 270;

/** The parts of the configuration the webview needs. */
export interface ViewerSettings {
  defaultZoom: FitMode;
  background: 'editor' | 'white' | 'gray';
  invertColors: 'never' | 'always' | 'auto';
  textLayer: boolean;
  links: boolean;
  outlineVisible: boolean;
  outlineWidth: number;
  maxCanvasPixels: number;
  renderAhead: number;
}

/**
 * Where the page gets the document's bytes.
 *
 * `url` is the fast path and the usual one: a `vscode-resource` URL the webview
 * streams through VSCode's own resource server, so a 200 MB file never crosses
 * the extension host. `bytes` is for documents that have no such URL — anything
 * outside the file system, served by a virtual file-system provider — which
 * arrive base64-chunked over `postMessage` instead.
 */
export type DocumentSource =
  | { kind: 'url'; url: string }
  | { kind: 'bytes'; byteLength: number };

/**
 * Where pdf.js's out-of-bundle data lives, as webview URLs.
 *
 * Only the host can build these: turning an extension path into something a
 * webview may load is `Webview.asWebviewUri`'s job, and the page has no way to
 * guess the answer.
 */
export interface PdfAssetUrls {
  /** The worker bundle, fetched and re-hosted as a blob by the page. */
  worker: string;
  /** Adobe's predefined CMaps — CID-keyed fonts, which is most CJK. */
  cMap: string;
  /** The 14 standard fonts, for documents that embed none. */
  standardFont: string;
  /** JBIG2 / JPEG 2000 / ICC decoders. */
  wasm: string;
}

/** What the toolbar, the title bar, and the palette can all ask for. */
export type ViewerCommand =
  | 'nextPage'
  | 'previousPage'
  | 'goToPage'
  | 'zoomIn'
  | 'zoomOut'
  | 'zoomReset'
  | 'fitWidth'
  | 'fitPage'
  | 'rotateClockwise'
  | 'rotateCounterclockwise'
  | 'toggleOutline'
  | 'toggleInvertColors'
  | 'find'
  | 'exportPagePng';

export const VIEWER_COMMANDS: readonly ViewerCommand[] = [
  'nextPage',
  'previousPage',
  'goToPage',
  'zoomIn',
  'zoomOut',
  'zoomReset',
  'fitWidth',
  'fitPage',
  'rotateClockwise',
  'rotateCounterclockwise',
  'toggleOutline',
  'toggleInvertColors',
  'find',
  'exportPagePng',
];

/** Where a reader was, so a reopen or a reload can put them back. */
export interface ViewerPlace {
  /** 1-based. */
  page: number;
  zoom: number;
  fit: FitMode;
  rotation: Rotation;
  inverted: boolean;
  outlineVisible: boolean;
  outlineWidth: number;
  /** How far into `page` the viewport was, as a fraction of its height. */
  offsetRatio: number;
}

/** Host → webview. */
export type HostToWebview =
  | {
      type: 'open';
      name: string;
      source: DocumentSource;
      assets: PdfAssetUrls;
      settings: ViewerSettings;
      /** Absent for a document this window has not shown before. */
      restore?: ViewerPlace;
    }
  /** One slice of the `bytes` path, in order, base64. */
  | { type: 'chunk'; index: number; total: number; data: string }
  /** The file changed on disk: same document, new bytes, same place. */
  | { type: 'reload'; source: DocumentSource }
  | { type: 'settings'; settings: ViewerSettings }
  | { type: 'command'; command: ViewerCommand; page?: number }
  | { type: 'hostError'; message: string };

/** Webview → host. */
export type WebviewToHost =
  | { type: 'ready' }
  /** The document opened: how many pages, and what it is called. */
  | { type: 'opened'; pageCount: number; title?: string }
  /** Throttled; drives the status bar and what a reopen restores. */
  | { type: 'place'; place: ViewerPlace }
  /** A link annotation with an external destination. The page never navigates. */
  | { type: 'openLink'; href: string }
  /** The answer to `exportPagePng`: one page, base64 PNG. */
  | { type: 'pagePng'; page: number; data: string }
  /** The resource URL did not load; fall back to shipping the bytes over. */
  | { type: 'needBytes'; reason: string }
  /** The document could not be opened at all — a bad file, or a wrong password. */
  | { type: 'failed'; message: string }
  | { type: 'error'; message: string; context: string };

/** What the webview persists across a window reload. */
export interface WebviewState {
  place: ViewerPlace;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isPageNumber(value: unknown): value is number {
  return isNumber(value) && Number.isInteger(value) && value >= 1 && value <= 1_000_000;
}

function isFit(value: unknown): value is FitMode {
  return value === 'fit-width' || value === 'fit-page' || value === 'actual';
}

function isRotation(value: unknown): value is Rotation {
  return value === 0 || value === 90 || value === 180 || value === 270;
}

function parsePlace(value: unknown): ViewerPlace | null {
  if (!isObject(value)) return null;
  if (!isPageNumber(value.page)) return null;
  if (!isNumber(value.zoom) || value.zoom <= 0 || value.zoom > 40) return null;
  if (!isFit(value.fit) || !isRotation(value.rotation)) return null;
  if (typeof value.inverted !== 'boolean') return null;
  if (typeof value.outlineVisible !== 'boolean') return null;
  if (!isNumber(value.outlineWidth) || value.outlineWidth < 0 || value.outlineWidth > 4000) {
    return null;
  }
  if (!isNumber(value.offsetRatio) || value.offsetRatio < -1 || value.offsetRatio > 2) {
    return null;
  }
  return {
    page: value.page,
    zoom: value.zoom,
    fit: value.fit,
    rotation: value.rotation,
    inverted: value.inverted,
    outlineVisible: value.outlineVisible,
    outlineWidth: value.outlineWidth,
    offsetRatio: value.offsetRatio,
  };
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

    case 'opened': {
      if (!isPageNumber(value.pageCount)) return null;
      const title = typeof value.title === 'string' ? value.title.slice(0, 300) : undefined;
      return { type: 'opened', pageCount: value.pageCount, title };
    }

    case 'place': {
      const place = parsePlace(value.place);
      return place ? { type: 'place', place } : null;
    }

    case 'openLink': {
      if (typeof value.href !== 'string' || value.href.length > 4096) return null;
      return { type: 'openLink', href: value.href };
    }

    case 'pagePng': {
      if (!isPageNumber(value.page)) return null;
      if (typeof value.data !== 'string') return null;
      // A PNG that would not survive `Buffer.from(…, 'base64')` intact is not
      // something we produced, and writing it would put a corrupt file on disk.
      if (!/^[A-Za-z0-9+/]*={0,2}$/.test(value.data) || value.data.length % 4 !== 0) {
        return null;
      }
      return { type: 'pagePng', page: value.page, data: value.data };
    }

    case 'needBytes': {
      if (typeof value.reason !== 'string') return null;
      return { type: 'needBytes', reason: value.reason.slice(0, 500) };
    }

    case 'failed': {
      if (typeof value.message !== 'string') return null;
      return { type: 'failed', message: value.message.slice(0, 2000) };
    }

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

/** Schemes the host will hand to `env.openExternal`. */
export const ALLOWED_LINK_SCHEMES = ['https:', 'http:', 'mailto:'];

/**
 * Whether a link out of a document may be opened externally.
 *
 * The webview never navigates; it asks, and this decides. A PDF's link
 * annotations are attacker-controlled strings, so `file:` — which would open
 * anything on the machine in whatever the OS has registered for it — is not on
 * the list.
 */
export function isAllowedLink(href: string): boolean {
  try {
    return ALLOWED_LINK_SCHEMES.includes(new URL(href).protocol);
  } catch {
    return false;
  }
}
