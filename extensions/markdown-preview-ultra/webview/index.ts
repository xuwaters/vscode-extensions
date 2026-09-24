import './styles/preview.css';
import './styles/frontmatter.css';
import './styles/highlight.css';
import './styles/math.css';
import './styles/mermaid.css';
import './styles/themes/github-light.css';
import './styles/themes/github-dark.css';
import 'katex/dist/katex.min.css';

import type {
  CodeBlockPrefs,
  HostToWebview,
  PreviewFont,
  PreviewOverrides,
  PreviewSettings,
  PreviewTheme,
  UpdateMessage,
  WebviewToHost,
} from '../src/messages';
import { applyPatches } from './patch';
import {
  buildScrollMap,
  lineForOffset,
  offsetForLine,
  sourceposLine,
  type MapEntry,
} from './scrollMap';
import { highlightCode } from './postprocess/highlight';
import { renderMath } from './postprocess/katex';
import { renderMermaid, type ResolvedMermaidTheme } from './postprocess/mermaid';
import {
  addCodeActions,
  applyCodeBlockPrefs,
  installCodeActions,
  readStampedCodeBlockPrefs,
} from './ui/codeBlocks';
import { installEditButton } from './ui/editButton';
import { FontToggle } from './ui/fontToggle';
import { renderFrontmatter } from './ui/frontmatter';
import { installLightbox } from './ui/lightbox';
import { NavButtons } from './ui/nav';
import { ThemeToggle } from './ui/themeToggle';
import { TocSidebar, addHeadingAnchors } from './ui/toc';
import { installZoom } from './ui/zoom';

interface PersistedState {
  uri?: string;
  tocVisible?: boolean;
  /** Dragged sidebar width; wins over the configured default. */
  tocWidth?: number;
  zoom?: number;
}

declare function acquireVsCodeApi(): {
  postMessage(message: WebviewToHost): void;
  getState(): PersistedState | undefined;
  setState(state: PersistedState): void;
};

const vscode = acquireVsCodeApi();
const content = document.getElementById('content') as HTMLElement;
const frontmatterContainer = document.getElementById(
  'frontmatter',
) as HTMLElement;

let state: PersistedState = vscode.getState() ?? {};
let settings: PreviewSettings | null = null;
let lastSeq = 0;

function saveState(patch: Partial<PersistedState>): void {
  state = { ...state, ...patch };
  vscode.setState(state);
}

// ── Scroll sync ──────────────────────────────────────────────────────

/** Ignore reciprocal sync events briefly after a programmatic scroll. */
let suppressScrollUntil = 0;
let scrollMap: MapEntry[] | null = null;
let revealThrottle: ReturnType<typeof setTimeout> | undefined;
/** Whether the panel is the on-screen tab of its group — see `setHostVisible`. */
let hostVisible = true;

function invalidateScrollMap(): void {
  scrollMap = null;
}

function getScrollMap(): MapEntry[] {
  if (scrollMap) return scrollMap;
  const map = buildScrollMap(content);
  // Off screen every element measures zero. Such a map is useless but it must
  // above all not outlive the panel's return, so it is never cached.
  if (hostVisible) scrollMap = map;
  return map;
}

function scrollToOffset(top: number): void {
  if (window.scrollY === top) return;
  suppressScrollUntil = Date.now() + 150;
  window.scrollTo({ top });
}

function scrollToLine(line: number, ratio: number): void {
  const top = offsetForLine(getScrollMap(), line);
  if (top === null) return;
  scrollToOffset(Math.max(0, top - ratio * window.innerHeight - 8));
}

/**
 * Apply a scroll on the next frame, keeping only the last request. A panel
 * that just came back on screen has not necessarily been laid out yet, and
 * where the host wants us supersedes the offset we restore on our own.
 */
let queuedScroll: (() => void) | null = null;
let queuedFrame = 0;

function queueScroll(apply: () => void): void {
  queuedScroll = apply;
  if (queuedFrame) return;
  queuedFrame = requestAnimationFrame(() => {
    queuedFrame = 0;
    const run = queuedScroll;
    queuedScroll = null;
    run?.();
  });
}

window.addEventListener(
  'scroll',
  () => {
    if (Date.now() < suppressScrollUntil) return;
    if (!hostVisible) return;
    if (!settings?.scrollSync) return;
    if (revealThrottle) return;
    revealThrottle = setTimeout(() => {
      revealThrottle = undefined;
      const line = lineForOffset(getScrollMap(), window.scrollY + 8);
      if (line !== null) vscode.postMessage({ type: 'revealLine', line });
    }, 100);
  },
  { passive: true },
);

window.addEventListener('resize', invalidateScrollMap);
// Late-loading images shift every offset below them.
content.addEventListener('load', invalidateScrollMap, true);

// ── Visibility ───────────────────────────────────────────────────────

/** Where the page stood when it went off screen, in case the browser clamps it. */
let parkedScrollY: number | null = null;

/**
 * Track whether the panel is the on-screen tab of its group.
 * `retainContextWhenHidden` keeps this page alive behind another tab but not
 * its layout: measurements read zero and the browser is free to clamp the
 * scroll position to the top. So the page stops measuring while it is away and
 * puts the reader back where they were on return — the host lands the editor's
 * position on top of that when the two are meant to be in sync.
 */
function setHostVisible(visible: boolean): void {
  if (visible === hostVisible) return;
  hostVisible = visible;
  invalidateScrollMap();
  if (!visible) {
    parkedScrollY = window.scrollY;
    return;
  }
  const top = parkedScrollY;
  parkedScrollY = null;
  if (top !== null && top > 0) queueScroll(() => scrollToOffset(top));
}

// ── Theme ────────────────────────────────────────────────────────────

/**
 * The in-page light/dark switch, which wins over the configured theme until it
 * is cleared. The host holds it for the whole window — this page is told what
 * it stands at and never decides on its own, which is what carries a flip from
 * one file to the next.
 */
let themeOverride: PreviewTheme | null = null;

/**
 * The theme the host stamped onto <body> when it built the page, switch
 * included. It stands in for the settings until the first `update` arrives, so
 * the page never repaints away from what it was served in.
 */
const stampedTheme = readStampedTheme();

function readStampedTheme(): PreviewTheme {
  const cls = document.body.classList;
  if (cls.contains('theme-github-light')) return 'github-light';
  if (cls.contains('theme-github-dark')) return 'github-dark';
  return 'auto';
}

function effectiveTheme(): PreviewTheme {
  return themeOverride ?? settings?.theme ?? stampedTheme;
}

function themeKind(): 'light' | 'dark' {
  const theme = effectiveTheme();
  if (theme === 'github-light') return 'light';
  if (theme === 'github-dark') return 'dark';
  const cls = document.body.classList;
  return cls.contains('vscode-light') ||
    cls.contains('vscode-high-contrast-light')
    ? 'light'
    : 'dark';
}

function resolvedMermaidTheme(): ResolvedMermaidTheme {
  const configured = settings?.mermaidTheme ?? 'auto';
  if (configured !== 'auto') return configured;
  return themeKind() === 'dark' ? 'dark' : 'default';
}

function applyThemeClasses(): void {
  const theme = effectiveTheme();
  const cls = document.body.classList;
  cls.toggle('theme-github-light', theme === 'github-light');
  cls.toggle('theme-github-dark', theme === 'github-dark');
  themeToggle.update(themeKind(), themeOverride !== null);
}

/** Flip the page between light and dark without touching the configuration. */
function toggleTheme(): void {
  const next = themeKind() === 'light' ? 'github-dark' : 'github-light';
  // The host owns the switch: it decides whether this is an override or a
  // return to the configured theme, records it for the window, and tells every
  // open preview. Painting it here first is only what keeps the click instant —
  // its answer lands on top.
  themeOverride = next;
  applyThemeClasses();
  rerenderAllMermaid();
  vscode.postMessage({ type: 'setTheme', theme: next });
}

/** The host's word on the switch: our own flip echoed, or another page's. */
function setThemeOverride(theme: PreviewTheme | null): void {
  if (theme === themeOverride) return;
  const before = themeKind();
  themeOverride = theme;
  applyThemeClasses();
  if (themeKind() !== before) rerenderAllMermaid();
}

// ── Font ─────────────────────────────────────────────────────────────

/**
 * The in-page prose/monospace switch. Held by the host for the window exactly
 * as the light/dark one is, so a reader who asks for the editor's font gets it
 * in the next file too.
 */
let fontOverride: PreviewFont | null = null;

/** The font the host stamped onto <body> when it built the page. */
const stampedFont: PreviewFont = document.body.classList.contains('font-mono')
  ? 'monospace'
  : 'proportional';

function effectiveFont(): PreviewFont {
  return fontOverride ?? settings?.font ?? stampedFont;
}

function applyFontClass(): void {
  const font = effectiveFont();
  const mono = font === 'monospace';
  fontToggle.update(font, fontOverride !== null);
  if (mono === document.body.classList.contains('font-mono')) return;
  holdLineAcross(() => document.body.classList.toggle('font-mono', mono));
}

/**
 * Every offset on the page is measured against how it is laid out, and a
 * restyle that reflows the page moves all of them: hold the line being read
 * across the reflow rather than letting the page slide out from under it.
 */
function holdLineAcross(restyle: () => void): void {
  const line = hostVisible
    ? lineForOffset(getScrollMap(), window.scrollY + 8)
    : null;
  restyle();
  invalidateScrollMap();
  if (line !== null) queueScroll(() => scrollToLine(line, 0));
}

/** Flip the page between the reading font and the editor's own. */
function toggleFont(): void {
  const next: PreviewFont =
    effectiveFont() === 'monospace' ? 'proportional' : 'monospace';
  // As with the theme, the host owns the switch: it decides whether this is an
  // override or a return to the configured font, records it for the window, and
  // tells every open preview. Painting it here first only keeps the click
  // instant — its answer lands on top.
  fontOverride = next;
  applyFontClass();
  vscode.postMessage({ type: 'setFont', font: next });
}

/** The host's word on the switch: our own flip echoed, or another page's. */
function setFontOverride(font: PreviewFont | null): void {
  if (font === fontOverride) return;
  fontOverride = font;
  applyFontClass();
}

/** Where both switches stand, as the host holds them. */
function setOverrides(overrides: PreviewOverrides): void {
  setThemeOverride(overrides.theme);
  setFontOverride(overrides.font);
}

// ── Code blocks ──────────────────────────────────────────────────────

/**
 * Soft wrap and line numbers for every fence. Held by the host for the window
 * like the switches above, so the next file opens the way this one was left.
 */
let codeBlocks: CodeBlockPrefs = readStampedCodeBlockPrefs();

function setCodeBlocks(prefs: CodeBlockPrefs): void {
  if (
    prefs.wrap === codeBlocks.wrap &&
    prefs.lineNumbers === codeBlocks.lineNumbers
  ) {
    return;
  }
  codeBlocks = prefs;
  holdLineAcross(() => applyCodeBlockPrefs(prefs));
}

/** A fence's Wrap or Lines button: paint it now, and let the host record it. */
function toggleCodeBlocks(next: CodeBlockPrefs): void {
  setCodeBlocks(next);
  vscode.postMessage({ type: 'setCodeBlocks', codeBlocks: next });
}

// ── Document chrome (base href, custom styles) ───────────────────────

function ensureBase(href: string): void {
  let base = document.head.querySelector('base');
  if (!base) {
    base = document.createElement('base');
    document.head.prepend(base);
  }
  if (base.href !== href) base.href = href;
}

function ensureCustomStyles(urls: string[]): void {
  const existing = document.head.querySelectorAll<HTMLLinkElement>(
    'link[data-custom-css]',
  );
  const current = Array.from(existing).map((l) => l.href);
  if (current.length === urls.length && current.every((u, i) => u === urls[i])) {
    return;
  }
  for (const link of existing) link.remove();
  for (const url of urls) {
    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = url;
    link.dataset.customCss = '';
    document.head.append(link);
  }
}

// ── UI chrome ────────────────────────────────────────────────────────

/** Floating chip bar in the top-right corner: history, edit, theme, TOC toggle. */
const toolbar = document.createElement('div');
toolbar.id = 'preview-toolbar';
document.body.append(toolbar);

const nav = new NavButtons(toolbar, (direction) =>
  vscode.postMessage({ type: 'navigate', direction }),
);

installEditButton(toolbar, () => vscode.postMessage({ type: 'openSource' }));

const themeToggle = new ThemeToggle(toolbar, toggleTheme);

const fontToggle = new FontToggle(toolbar, toggleFont);

const toc = new TocSidebar(
  toolbar,
  (entry) => {
    const el = document.getElementById(entry.slug);
    if (el) {
      suppressScrollUntil = Date.now() + 300;
      el.scrollIntoView({ behavior: 'smooth' });
    }
    vscode.postMessage({ type: 'revealLine', line: entry.line });
  },
  (visible) => saveState({ tocVisible: visible }),
  (width) => saveState({ tocWidth: width }),
);
if (state.tocWidth !== undefined) toc.setWidth(state.tocWidth);

installCodeActions(content, () => codeBlocks, toggleCodeBlocks);
installLightbox(content);
installZoom(content, state.zoom ?? 1, (zoom) => {
  saveState({ zoom });
  invalidateScrollMap();
});

// A restored override must land before the first paint; the host stamped the
// *configured* theme and font onto <body> when it built the document.
applyThemeClasses();
applyFontClass();

// ── Updates ──────────────────────────────────────────────────────────

function handleUpdate(msg: UpdateMessage): void {
  if (msg.seq <= lastSeq && !msg.reset) return;
  lastSeq = msg.seq;
  settings = msg.settings;
  setOverrides(msg.overrides);
  setCodeBlocks(msg.codeBlocks);

  ensureBase(msg.baseHref);
  ensureCustomStyles(msg.customStyles);
  applyThemeClasses();
  applyFontClass();
  nav.update(msg.canGoBack, msg.canGoForward);
  const sameDocument = state.uri === msg.uri;
  if (!sameDocument) parkedScrollY = null;
  saveState({ uri: msg.uri });

  let restoreLine: number | null = null;
  if (msg.reset) {
    // Keep the reader's place across a full rebuild — but a *different*
    // document (followed a link, went back) starts at its top. Off screen
    // there is nothing to measure with; the parked offset covers that case.
    restoreLine =
      sameDocument && hostVisible
        ? lineForOffset(getScrollMap(), window.scrollY + 8)
        : null;
    content.textContent = '';
  }

  let changed: Element[];
  try {
    changed = applyPatches(content, msg.patches);
  } catch (err) {
    vscode.postMessage({
      type: 'error',
      message: err instanceof Error ? err.message : String(err),
      context: 'patch-applier',
    });
    return;
  }
  invalidateScrollMap();

  postprocess(changed);
  renderFrontmatter(
    frontmatterContainer,
    msg.frontmatter,
    settings.frontmatterDisplay,
  );
  toc.update(msg.toc);
  toc.setWidth(state.tocWidth ?? settings.tocWidth);
  if (state.tocVisible === undefined) {
    toc.setVisible(settings.tocVisible, false);
  } else {
    toc.setVisible(state.tocVisible, false);
  }

  if (restoreLine !== null) {
    const line = restoreLine;
    queueScroll(() => scrollToLine(line, 0));
  } else if (msg.reset && !sameDocument) {
    queueScroll(() => scrollToOffset(0));
  }
}

function postprocess(changed: Element[]): void {
  if (changed.length === 0) return;
  if (settings?.math) renderMath(changed);
  highlightCode(changed);
  addCodeActions(changed);
  addHeadingAnchors(changed);
  enableTaskCheckboxes(changed);
  if (settings?.mermaid) {
    void renderMermaid(changed, resolvedMermaidTheme()).then(
      invalidateScrollMap,
    );
  }
}

/** Re-render every mermaid diagram (theme change). */
function rerenderAllMermaid(): void {
  if (!settings?.mermaid) return;
  void renderMermaid([content], resolvedMermaidTheme()).then(
    invalidateScrollMap,
  );
}

function enableTaskCheckboxes(roots: Element[]): void {
  if (!settings?.taskToggle) return;
  for (const root of roots) {
    for (const input of root.querySelectorAll<HTMLInputElement>(
      'li input[type="checkbox"][disabled]',
    )) {
      input.disabled = false;
    }
  }
}

function showNoEngine(): void {
  content.innerHTML = `<div class="engine-missing">
    <h2>Preview engine not built</h2>
    <p>The WASM rendering engine is missing. Run
    <code>pnpm run build:wasm</code> in
    <code>extensions/markdown-preview-ultra</code>, then reload the window.</p>
  </div>`;
}

// ── Message loop ─────────────────────────────────────────────────────

window.addEventListener('message', (event) => {
  const msg = event.data as HostToWebview;
  switch (msg.type) {
    case 'update':
      handleUpdate(msg);
      break;
    case 'scroll':
      if (settings?.scrollSync && hostVisible) {
        queueScroll(() => scrollToLine(msg.line, msg.ratio));
      }
      break;
    case 'visibility':
      setHostVisible(msg.visible);
      break;
    case 'theme':
      // Only matters while following the editor, but the switch's icon tracks
      // whatever the page actually shows.
      applyThemeClasses();
      rerenderAllMermaid();
      break;
    case 'overrides':
      setOverrides(msg.overrides);
      setCodeBlocks(msg.codeBlocks);
      break;
    case 'noEngine':
      showNoEngine();
      break;
  }
});

// ── Interaction ──────────────────────────────────────────────────────

content.addEventListener('click', (event) => {
  const target = event.target as HTMLElement;

  // Task checkboxes (the one write path; opt-in).
  if (target instanceof HTMLInputElement && target.type === 'checkbox') {
    const li = target.closest('li[data-sourcepos]');
    const line = sourceposLine(li?.getAttribute('data-sourcepos') ?? null);
    if (!settings?.taskToggle || line === null) {
      event.preventDefault();
      return;
    }
    vscode.postMessage({ type: 'toggleTask', line, checked: target.checked });
    return;
  }

  const anchor = target.closest('a');
  if (!anchor) return;
  const href = anchor.getAttribute('href');
  if (!href) return;

  event.preventDefault();
  // VSCode's own webview shell listens for link clicks on <body> and hands
  // `anchor.href` — resolved against <base>, so a vscode-cdn.net URL — to the
  // browser. `preventDefault` does not deter it; not reaching it does.
  event.stopPropagation();
  if (anchor.classList.contains('heading-anchor')) {
    void navigator.clipboard.writeText(href);
  }
  if (href.startsWith('#')) {
    const el = document.getElementById(href.slice(1));
    if (el) {
      suppressScrollUntil = Date.now() + 300;
      el.scrollIntoView({ behavior: 'smooth' });
    }
    return;
  }
  vscode.postMessage({ type: 'openLink', href });
});

// Double-click any block → jump to its source line in the editor.
content.addEventListener('dblclick', (event) => {
  const target = event.target as HTMLElement;
  if (target.closest('a, button, input, textarea')) return;
  const block = target.closest('[data-sourcepos]');
  const line = sourceposLine(block?.getAttribute('data-sourcepos') ?? null);
  if (line === null) return;
  vscode.postMessage({ type: 'jumpToLine', line });
});

// Signal readiness; the host replies with the first `update`.
vscode.postMessage({ type: 'ready' });
