import './styles/preview.css';
import './styles/frontmatter.css';
import './styles/highlight.css';
import './styles/math.css';
import './styles/mermaid.css';
import './styles/themes/github-light.css';
import './styles/themes/github-dark.css';
import 'katex/dist/katex.min.css';

import type {
  HostToWebview,
  PreviewSettings,
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
import { addCopyButtons, installCopyHandler } from './ui/copyCode';
import { renderFrontmatter } from './ui/frontmatter';
import { installLightbox } from './ui/lightbox';
import { TocSidebar, addHeadingAnchors } from './ui/toc';
import { installZoom } from './ui/zoom';

interface PersistedState {
  uri?: string;
  tocVisible?: boolean;
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

function invalidateScrollMap(): void {
  scrollMap = null;
}

function getScrollMap(): MapEntry[] {
  if (!scrollMap) scrollMap = buildScrollMap(content);
  return scrollMap;
}

function scrollToLine(line: number, ratio: number): void {
  const top = offsetForLine(getScrollMap(), line);
  if (top === null) return;
  suppressScrollUntil = Date.now() + 150;
  window.scrollTo({ top: Math.max(0, top - ratio * window.innerHeight - 8) });
}

window.addEventListener(
  'scroll',
  () => {
    if (Date.now() < suppressScrollUntil) return;
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

// ── Theme ────────────────────────────────────────────────────────────

function themeKind(): 'light' | 'dark' {
  if (settings?.theme === 'github-light') return 'light';
  if (settings?.theme === 'github-dark') return 'dark';
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
  document.body.classList.toggle(
    'theme-github-light',
    settings?.theme === 'github-light',
  );
  document.body.classList.toggle(
    'theme-github-dark',
    settings?.theme === 'github-dark',
  );
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

const toc = new TocSidebar(
  (entry) => {
    const el = document.getElementById(entry.slug);
    if (el) {
      suppressScrollUntil = Date.now() + 300;
      el.scrollIntoView({ behavior: 'smooth' });
    }
    vscode.postMessage({ type: 'revealLine', line: entry.line });
  },
  (visible) => saveState({ tocVisible: visible }),
);

installCopyHandler(content);
installLightbox(content);
installZoom(content, state.zoom ?? 1, (zoom) => {
  saveState({ zoom });
  invalidateScrollMap();
});

// ── Updates ──────────────────────────────────────────────────────────

function handleUpdate(msg: UpdateMessage): void {
  if (msg.seq <= lastSeq && !msg.reset) return;
  lastSeq = msg.seq;
  settings = msg.settings;

  ensureBase(msg.baseHref);
  ensureCustomStyles(msg.customStyles);
  applyThemeClasses();
  saveState({ uri: msg.uri });

  let restoreLine: number | null = null;
  if (msg.reset) {
    // Keep the reader's place across a full rebuild.
    restoreLine = lineForOffset(getScrollMap(), window.scrollY + 8);
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
  if (state.tocVisible === undefined) {
    toc.setVisible(settings.tocVisible, false);
  } else {
    toc.setVisible(state.tocVisible, false);
  }

  if (restoreLine !== null) {
    const line = restoreLine;
    requestAnimationFrame(() => scrollToLine(line, 0));
  }
}

function postprocess(changed: Element[]): void {
  if (changed.length === 0) return;
  if (settings?.math) renderMath(changed);
  highlightCode(changed);
  addCopyButtons(changed);
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
      if (settings?.scrollSync) scrollToLine(msg.line, msg.ratio);
      break;
    case 'theme':
      rerenderAllMermaid();
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
