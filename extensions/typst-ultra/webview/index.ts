import type {
  FitMode,
  HostToWebview,
  PreviewSettings,
  WebviewState,
  WebviewToHost,
} from '../src/preview/messages.js';
import { PageList } from './pageList.js';
import styles from './styles/preview.css';

/**
 * The preview webview.
 *
 * Dependency-free and small on purpose: everything expensive — layout,
 * rendering, position mapping — already happened in the compiler.
 */

interface VsCodeApi {
  postMessage(message: WebviewToHost): void;
  getState(): WebviewState | undefined;
  setState(state: WebviewState): void;
}

declare function acquireVsCodeApi(): VsCodeApi;

const vscode = acquireVsCodeApi();

injectStyles();

const container = must<HTMLElement>('pages');
const statusBar = must<HTMLElement>('status');
const zoomLabel = must<HTMLElement>('zoom-level');
const pageCount = must<HTMLElement>('page-count');
const goToPage = must<HTMLInputElement>('go-to-page');

let settings: PreviewSettings = {
  scrollSync: 'both',
  cursorIndicator: true,
  invertColors: 'never',
  background: 'editor',
  renderMode: 'svg',
};

const restored = vscode.getState();
let fit: FitMode = restored?.fit ?? 'width';
let inverted = restored?.inverted ?? false;
/** Highest `seq` applied. Anything older is a superseded compile. */
let appliedSeq = 0;
/** The document on screen, so a change of subject can be told from an edit. */
let shownUri: string | undefined;
let scrollTimer: ReturnType<typeof setTimeout> | undefined;

const pages = new PageList(
  container,
  (first, last, known) =>
    post({ type: 'viewport', first, last, known, zoom: pages.scale }),
  (page, xPt, yPt) => post({ type: 'click', page, xPt, yPt }),
);

if (restored?.zoom) pages.setZoom(restored.zoom);

window.addEventListener('message', (event: MessageEvent<unknown>) => {
  const message = event.data as HostToWebview;
  if (typeof message !== 'object' || message === null) return;

  try {
    handle(message);
  } catch (error) {
    post({
      type: 'error',
      message: error instanceof Error ? error.message : String(error),
      context: message.type ?? 'unknown',
    });
  }
});

function handle(message: HostToWebview): void {
  switch (message.type) {
    case 'init':
    case 'settings':
      applySettings(message.settings);
      break;

    case 'metrics':
      // A message from a superseded compile can never overwrite a newer one.
      if (message.seq <= appliedSeq) return;
      appliedSeq = message.seq;
      // The panel follows the active editor, so a new URI here means the reader
      // opened a different document — not that this one changed.
      if (shownUri !== undefined && shownUri !== message.uri) pages.reset();
      shownUri = message.uri;
      pages.setMetrics(message.pages);
      pageCount.textContent = `/ ${pages.length}`;
      goToPage.max = String(Math.max(1, pages.length));
      if (fit !== 'actual') applyFit(fit);
      break;

    case 'pages':
      if (message.seq <= appliedSeq) return;
      appliedSeq = message.seq;
      pages.applyPatches(message.patches);
      break;

    case 'cursor':
      showCursor(message.page, message.yPt);
      break;

    case 'status':
      showStatus(message.state, message.message);
      break;

    case 'goToPage':
      if (message.page >= 0) pages.goToPage(message.page);
      else toggleInvert();
      break;
  }
}

// ── Chrome ───────────────────────────────────────────────────────────────────

must<HTMLButtonElement>('zoom-in').addEventListener('click', () => setZoom(pages.scale * 1.2));
must<HTMLButtonElement>('zoom-out').addEventListener('click', () => setZoom(pages.scale / 1.2));
must<HTMLButtonElement>('fit-width').addEventListener('click', () => applyFit('width'));
must<HTMLButtonElement>('fit-page').addEventListener('click', () => applyFit('page'));
must<HTMLButtonElement>('invert').addEventListener('click', () => toggleInvert());

// Leaving the page: the host decides what each of these means for the surface
// it is showing — a panel hands focus to the editor beside it, a full-tab
// preview hands the tab itself back.
must<HTMLButtonElement>('edit-source').addEventListener('click', () =>
  post({ type: 'openSource' }),
);
must<HTMLButtonElement>('export').addEventListener('click', () =>
  post({ type: 'export' }),
);

goToPage.addEventListener('change', () => {
  const page = Number(goToPage.value) - 1;
  if (Number.isInteger(page) && page >= 0) pages.goToPage(page);
});

window.addEventListener('keydown', (event) => {
  if (!event.ctrlKey && !event.metaKey) return;

  if (event.key === '+' || event.key === '=') {
    setZoom(pages.scale * 1.2);
    event.preventDefault();
  } else if (event.key === '-') {
    setZoom(pages.scale / 1.2);
    event.preventDefault();
  } else if (event.key === '0') {
    applyFit('actual');
    event.preventDefault();
  }
});

container.addEventListener(
  'scroll',
  () => {
    updateCurrentPage();

    if (settings.scrollSync !== 'both' && settings.scrollSync !== 'previewToEditor') {
      return;
    }
    if (scrollTimer) clearTimeout(scrollTimer);
    scrollTimer = setTimeout(() => {
      const center = pages.centerPage();
      if (center) post({ type: 'scrolled', page: center.page, yPt: center.yPt });
    }, 120);
  },
  { passive: true },
);

// Links inside the SVG are handled here rather than by navigating: the webview
// never leaves the page.
container.addEventListener('click', (event) => {
  const target = event.target;
  if (!(target instanceof Element)) return;
  const anchor = target.closest('a');
  const href = anchor?.getAttribute('href');
  if (!href || href.startsWith('#')) return;

  event.preventDefault();
  post({ type: 'openLink', href });
});

post({ type: 'ready' });
applyInversion();
applyBackground();

// ── Helpers ──────────────────────────────────────────────────────────────────

function applySettings(next: PreviewSettings): void {
  const rasterBefore = settings.renderMode !== 'svg';
  settings = next;
  applyInversion();
  applyBackground();

  // Switching into or out of a raster mode invalidates what the client holds,
  // because the same page hash now has to arrive in a different format.
  if (rasterBefore !== (settings.renderMode !== 'svg')) pages.forget();
}

function setZoom(zoom: number): void {
  fit = 'actual';
  const before = pages.scale;
  pages.setZoom(zoom);
  zoomLabel.textContent = `${Math.round(pages.scale * 100)}%`;

  // A raster page is rendered at a fixed resolution, so a zoom step needs it
  // re-rendered or it goes soft. Vector pages just scale.
  if (settings.renderMode !== 'svg' && zoomStep(before) !== zoomStep(pages.scale)) {
    pages.forget();
  }
  saveState();
}

/** Zoom bucketed to halves, so small nudges do not re-render every page. */
function zoomStep(zoom: number): number {
  return Math.round(zoom * 2);
}

function applyFit(mode: FitMode): void {
  fit = mode;
  const zoom = pages.fit(mode);
  zoomLabel.textContent = `${Math.round(zoom * 100)}%`;
  saveState();
}

function toggleInvert(): void {
  inverted = !inverted;
  applyInversion();
  saveState();
}

function applyInversion(): void {
  const on =
    inverted ||
    settings.invertColors === 'always' ||
    (settings.invertColors === 'auto' && prefersDark());
  document.body.classList.toggle('inverted', on);
}

function prefersDark(): boolean {
  return document.body.classList.contains('vscode-dark') ||
    document.body.classList.contains('vscode-high-contrast');
}

function applyBackground(): void {
  document.body.dataset.background = settings.background;
}

function showStatus(state: 'compiling' | 'ok' | 'error', message?: string): void {
  // On failure the last good pages stay visible, dimmed, with a banner. The
  // preview never goes blank mid-edit — a transient syntax error while typing
  // would otherwise destroy it on every character.
  document.body.classList.toggle('has-error', state === 'error');

  if (state === 'ok') {
    statusBar.hidden = true;
    return;
  }
  statusBar.hidden = false;
  statusBar.textContent =
    state === 'compiling'
      ? 'Compiling…'
      : (message ?? 'The document has errors — showing the last good version');
  statusBar.className = `status status-${state}`;
}

function showCursor(page: number, yPt: number): void {
  const element = pages.reveal(page, yPt);
  if (!element || !settings.cursorIndicator) return;

  const marker = document.createElement('div');
  marker.className = 'cursor-indicator';
  marker.style.top = `${yPt * (96 / 72) * pages.scale}px`;
  element.append(marker);

  // Fades out via CSS animation; remove it once it has.
  setTimeout(() => marker.remove(), 1600);
}

function updateCurrentPage(): void {
  const center = pages.centerPage();
  if (center) goToPage.value = String(center.page + 1);
}

function saveState(): void {
  vscode.setState({
    zoom: pages.scale,
    fit,
    inverted,
    scrollTop: container.scrollTop,
  });
}

function post(message: WebviewToHost): void {
  vscode.postMessage(message);
}

function must<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`the preview is missing #${id}`);
  return element as T;
}

function injectStyles(): void {
  const style = document.createElement('style');
  style.textContent = styles;
  document.head.append(style);
}
