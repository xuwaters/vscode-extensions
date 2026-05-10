import type {
  FilterRule,
  HostToWebview,
  ParsedLines,
  ViewState,
  WebviewToHost,
} from '../src/types.js';

declare const acquireVsCodeApi: () => {
  postMessage(message: WebviewToHost): void;
  setState(state: unknown): void;
  getState(): unknown;
};

const vscode = acquireVsCodeApi();

const MIN_FONT = 8;
const MAX_FONT = 36;
const OVERSCAN = 12;

// === DOM handles ===
const scroller = byId<HTMLDivElement>('scroller');
const spacer = byId<HTMLDivElement>('spacer');
const viewport = byId<HTMLDivElement>('viewport');
const banner = byId<HTMLDivElement>('banner');
const info = byId<HTMLSpanElement>('info');
const searchInput = byId<HTMLInputElement>('search');
const searchInfo = byId<HTMLSpanElement>('search-info');
const chipsContainer = byId<HTMLSpanElement>('chips');
const btnAnsi = byId<HTMLButtonElement>('btn-ansi');
const btnWrap = byId<HTMLButtonElement>('btn-wrap');
const btnMode = byId<HTMLButtonElement>('btn-mode');
const btnRegex = byId<HTMLButtonElement>('btn-regex');
const btnCase = byId<HTMLButtonElement>('btn-case');
const btnFontUp = byId<HTMLButtonElement>('btn-font-up');
const btnFontDown = byId<HTMLButtonElement>('btn-font-down');
const btnFontReset = byId<HTMLButtonElement>('btn-font-reset');
const btnText = byId<HTMLButtonElement>('btn-text');

function byId<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) throw new Error(`#${id} not found`);
  return el as T;
}

// === State ===
let lines: ParsedLines = { html: [], text: [] };
let filterMatches: number[] = [];
let rules: FilterRule[] = [];
let view: ViewState = {
  renderAnsi: true,
  wordWrap: false,
  fontSize: 0,
  filterMode: 'highlight',
};
let totalBytes = 0;
let truncated = false;

let searchQuery = '';
let searchRegex = false;
let searchCase = false;
let searchMatcher: RegExp | null = null;

let visibleIndices: number[] = [];
let lineHeight = 18;
let scrollScheduled = false;

// === Rendering ===

function htmlEscape(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function lineBackground(idx: number): string | null {
  const m = filterMatches[idx];
  if (!m) return null;
  const rule = rules[m - 1];
  if (!rule || rule.enabled === false) return null;
  return rule.color ?? null;
}

function lineMatchesAnyEnabledFilter(idx: number): boolean {
  const m = filterMatches[idx];
  if (!m) return false;
  const rule = rules[m - 1];
  return !!rule && rule.enabled !== false;
}

function buildSearchMatcher(): RegExp | null {
  if (!searchQuery) return null;
  try {
    if (searchRegex) {
      return new RegExp(searchQuery, searchCase ? 'g' : 'gi');
    }
    const escaped = searchQuery.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    return new RegExp(escaped, searchCase ? 'g' : 'gi');
  } catch {
    return null;
  }
}

function lineMatchesSearch(idx: number): boolean {
  if (!searchMatcher) return true;
  const text = lines.text[idx] ?? '';
  searchMatcher.lastIndex = 0;
  return searchMatcher.test(text);
}

function recomputeVisibleIndices(): void {
  const total = lines.text.length;
  const out: number[] = [];
  const onlyMatching = view.filterMode === 'only-matching';
  const anyEnabledRule = rules.some((r) => r.enabled !== false);
  for (let i = 0; i < total; i++) {
    if (!lineMatchesSearch(i)) continue;
    if (onlyMatching && anyEnabledRule && !lineMatchesAnyEnabledFilter(i)) {
      continue;
    }
    out.push(i);
  }
  visibleIndices = out;
}

function applyLineContent(div: HTMLDivElement, idx: number): void {
  const html = view.renderAnsi
    ? lines.html[idx] ?? ''
    : htmlEscape(lines.text[idx] ?? '');
  if (searchMatcher && (lines.text[idx] ?? '').length > 0) {
    div.innerHTML = highlightSearchInHtml(html, lines.text[idx] ?? '');
  } else {
    div.innerHTML = html.length === 0 ? '​' : html;
  }
  const bg = lineBackground(idx);
  div.style.backgroundColor = bg ?? '';
  div.style.borderLeftColor = bg ?? 'transparent';
  div.dataset['i'] = String(idx);
}

/**
 * Inject `<mark>` around search-matching substrings while preserving the
 * existing ANSI-rendered HTML structure. Matches are computed against the
 * plain text and re-applied to the HTML by walking the rendered string in
 * tandem (entity-aware: `&amp;` advances 1 plain char, 5 HTML chars).
 */
function highlightSearchInHtml(html: string, plain: string): string {
  if (!searchMatcher) return html;
  searchMatcher.lastIndex = 0;
  const matches: Array<[number, number]> = [];
  let m: RegExpExecArray | null;
  while ((m = searchMatcher.exec(plain)) !== null) {
    if (m[0].length === 0) {
      searchMatcher.lastIndex++;
      continue;
    }
    matches.push([m.index, m.index + m[0].length]);
  }
  if (matches.length === 0) return html;

  // Walk html and plain in lockstep, inserting <mark>...</mark> around match ranges.
  let out = '';
  let plainPos = 0;
  let htmlPos = 0;
  let matchIdx = 0;
  let inMark = false;

  while (htmlPos < html.length) {
    // Skip over tags entirely; they don't consume plain-text characters.
    if (html[htmlPos] === '<') {
      const close = html.indexOf('>', htmlPos);
      const end = close === -1 ? html.length : close + 1;
      out += html.slice(htmlPos, end);
      htmlPos = end;
      continue;
    }
    // Determine the next "step": one plain character may correspond to 1 char
    // or to an HTML entity in the rendered form.
    let step = 1;
    if (html[htmlPos] === '&') {
      const semi = html.indexOf(';', htmlPos);
      if (semi !== -1 && semi - htmlPos <= 8) step = semi - htmlPos + 1;
    }

    // Open or close <mark> around match boundaries.
    while (
      matchIdx < matches.length &&
      plainPos === matches[matchIdx][0] &&
      !inMark
    ) {
      out += '<mark class="search-hit">';
      inMark = true;
    }

    out += html.slice(htmlPos, htmlPos + step);
    htmlPos += step;
    plainPos += 1;

    while (
      matchIdx < matches.length &&
      plainPos === matches[matchIdx][1] &&
      inMark
    ) {
      out += '</mark>';
      inMark = false;
      matchIdx++;
    }
  }
  if (inMark) out += '</mark>';
  return out;
}

// Pool of line elements reused across virtualization renders.
const linePool: HTMLDivElement[] = [];

function getLineEl(): HTMLDivElement {
  const el = linePool.pop();
  if (el) return el;
  const div = document.createElement('div');
  div.className = 'ln';
  return div;
}

function recycleLineEl(el: HTMLDivElement): void {
  el.style.transform = '';
  el.style.backgroundColor = '';
  el.style.borderLeftColor = '';
  linePool.push(el);
}

function measureLineHeight(): void {
  if (visibleIndices.length === 0) return;
  const probe = document.createElement('div');
  probe.className = 'ln';
  probe.style.visibility = 'hidden';
  probe.textContent = 'M';
  viewport.appendChild(probe);
  const r = probe.getBoundingClientRect();
  if (r.height > 0) lineHeight = r.height;
  viewport.removeChild(probe);
}

function renderVirtualized(): void {
  spacer.style.height = `${visibleIndices.length * lineHeight}px`;

  const scrollTop = scroller.scrollTop;
  const viewportHeight = scroller.clientHeight;
  const startVisIdx = Math.max(
    0,
    Math.floor(scrollTop / lineHeight) - OVERSCAN,
  );
  const endVisIdx = Math.min(
    visibleIndices.length,
    Math.ceil((scrollTop + viewportHeight) / lineHeight) + OVERSCAN,
  );

  // Reuse elements: keep the first `endVisIdx - startVisIdx` children, recycle the rest.
  const needed = endVisIdx - startVisIdx;
  while (viewport.children.length > needed) {
    const child = viewport.lastElementChild as HTMLDivElement | null;
    if (!child) break;
    viewport.removeChild(child);
    recycleLineEl(child);
  }
  while (viewport.children.length < needed) {
    viewport.appendChild(getLineEl());
  }

  for (let k = 0; k < needed; k++) {
    const visIdx = startVisIdx + k;
    const lineIdx = visibleIndices[visIdx];
    const el = viewport.children[k] as HTMLDivElement;
    el.style.transform = `translateY(${visIdx * lineHeight}px)`;
    el.style.position = 'absolute';
    el.style.left = '0';
    el.style.right = '0';
    applyLineContent(el, lineIdx);
  }
}

function renderWrap(): void {
  // Wrap mode: variable line heights, virtualization disabled. Render all
  // visible lines flow-positioned. For very large logs this is sluggish;
  // wrap mode is opt-in.
  spacer.style.height = '';
  // Replace children in a single pass using DocumentFragment.
  const frag = document.createDocumentFragment();
  for (let k = 0; k < visibleIndices.length; k++) {
    const lineIdx = visibleIndices[k];
    const el = document.createElement('div');
    el.className = 'ln';
    el.style.position = 'static';
    applyLineContent(el, lineIdx);
    frag.appendChild(el);
  }
  viewport.replaceChildren(frag);
}

function render(): void {
  if (view.wordWrap) renderWrap();
  else renderVirtualized();
}

function scheduleScrollRender(): void {
  if (scrollScheduled) return;
  scrollScheduled = true;
  requestAnimationFrame(() => {
    scrollScheduled = false;
    if (!view.wordWrap) renderVirtualized();
  });
}

// === UI bindings ===

function applyFontSize(): void {
  if (view.fontSize > 0) {
    document.body.style.fontSize = `${view.fontSize}px`;
  } else {
    document.body.style.fontSize = '';
  }
}

function applyToolbarState(): void {
  btnAnsi.classList.toggle('active', view.renderAnsi);
  btnWrap.classList.toggle('active', view.wordWrap);
  btnRegex.classList.toggle('active', searchRegex);
  btnCase.classList.toggle('active', searchCase);
  btnMode.textContent =
    view.filterMode === 'highlight' ? 'Highlight' : 'Only matching';
  btnMode.classList.toggle('active', view.filterMode === 'only-matching');
  document.body.classList.toggle('wrap', view.wordWrap);
  applyFontSize();
}

function renderChips(): void {
  chipsContainer.replaceChildren();
  rules.forEach((rule, i) => {
    const chip = document.createElement('span');
    chip.className = 'chip' + (rule.enabled === false ? ' off' : '');
    chip.title = `${rule.regex ? '/' : ''}${rule.pattern}${rule.regex ? '/' : ''}${rule.caseSensitive ? ' (case-sensitive)' : ''}`;
    if (rule.color) {
      const sw = document.createElement('span');
      sw.className = 'swatch';
      sw.style.background = rule.color;
      chip.appendChild(sw);
    }
    chip.appendChild(document.createTextNode(rule.name));
    chip.addEventListener('click', () => {
      const enabled = rule.enabled === false;
      vscode.postMessage({ type: 'setFilterEnabled', index: i, enabled });
    });
    chipsContainer.appendChild(chip);
  });
}

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function updateInfo(): void {
  const total = lines.text.length;
  const visible = visibleIndices.length;
  info.textContent = `${visible.toLocaleString()} / ${total.toLocaleString()} lines · ${fmtBytes(totalBytes)}`;
  banner.hidden = !truncated;
  if (truncated) {
    banner.textContent =
      'File exceeds logViewer.maxFileSizeBytes; only the head is shown. Open in Text Editor to see the full file.';
  }
  if (searchQuery) {
    let hits = 0;
    if (searchMatcher) {
      for (let i = 0; i < lines.text.length; i++) {
        searchMatcher.lastIndex = 0;
        if (searchMatcher.test(lines.text[i] ?? '')) hits++;
      }
    }
    searchInfo.textContent = `${hits} match${hits === 1 ? '' : 'es'}`;
  } else {
    searchInfo.textContent = '';
  }
}

function applyAll(): void {
  searchMatcher = buildSearchMatcher();
  recomputeVisibleIndices();
  measureLineHeight();
  applyToolbarState();
  scroller.scrollTop = 0;
  render();
  updateInfo();
}

function partialUpdate(opts: {
  remeasure?: boolean;
  preserveScroll?: boolean;
}): void {
  searchMatcher = buildSearchMatcher();
  recomputeVisibleIndices();
  if (opts.remeasure) measureLineHeight();
  applyToolbarState();
  if (!opts.preserveScroll) scroller.scrollTop = 0;
  render();
  updateInfo();
}

// === Message handling ===

window.addEventListener('message', (e: MessageEvent<HostToWebview>) => {
  const msg = e.data;
  switch (msg.type) {
    case 'init':
      lines = msg.lines;
      filterMatches = msg.filterMatches;
      rules = msg.rules;
      view = msg.state;
      truncated = msg.truncated;
      totalBytes = msg.totalBytes;
      renderChips();
      applyAll();
      break;
    case 'update':
      if (msg.lines) lines = msg.lines;
      if (msg.filterMatches) filterMatches = msg.filterMatches;
      if (msg.rules) {
        rules = msg.rules;
        renderChips();
      }
      if (msg.state) view = msg.state;
      if (msg.truncated !== undefined) truncated = msg.truncated;
      if (msg.totalBytes !== undefined) totalBytes = msg.totalBytes;
      partialUpdate({ remeasure: !!msg.lines, preserveScroll: !msg.lines });
      break;
    case 'focusSearch':
      searchInput.focus();
      searchInput.select();
      break;
    case 'commandToggle':
      if (msg.key === 'renderAnsi') {
        vscode.postMessage({
          type: 'setState',
          state: { renderAnsi: !view.renderAnsi },
        });
      } else if (msg.key === 'wordWrap') {
        vscode.postMessage({
          type: 'setState',
          state: { wordWrap: !view.wordWrap },
        });
      } else if (msg.key === 'filterMode') {
        vscode.postMessage({
          type: 'setState',
          state: {
            filterMode:
              view.filterMode === 'highlight' ? 'only-matching' : 'highlight',
          },
        });
      }
      break;
    case 'fontSizeCommand': {
      const current = view.fontSize > 0
        ? view.fontSize
        : parseFloat(getComputedStyle(document.body).fontSize) || 13;
      let next: number;
      if (msg.delta === 'reset') next = 0;
      else next = clamp(current + (msg.delta as number), MIN_FONT, MAX_FONT);
      vscode.postMessage({ type: 'setState', state: { fontSize: next } });
      break;
    }
  }
});

function clamp(n: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, n));
}

// === Toolbar handlers ===

btnAnsi.addEventListener('click', () =>
  vscode.postMessage({ type: 'setState', state: { renderAnsi: !view.renderAnsi } }),
);
btnWrap.addEventListener('click', () =>
  vscode.postMessage({ type: 'setState', state: { wordWrap: !view.wordWrap } }),
);
btnMode.addEventListener('click', () =>
  vscode.postMessage({
    type: 'setState',
    state: {
      filterMode: view.filterMode === 'highlight' ? 'only-matching' : 'highlight',
    },
  }),
);
btnText.addEventListener('click', () =>
  vscode.postMessage({ type: 'openInText' }),
);
btnFontUp.addEventListener('click', () => bumpFont(1));
btnFontDown.addEventListener('click', () => bumpFont(-1));
btnFontReset.addEventListener('click', () => setFont(0));
btnRegex.addEventListener('click', () => {
  searchRegex = !searchRegex;
  applyToolbarState();
  partialUpdate({ remeasure: false });
});
btnCase.addEventListener('click', () => {
  searchCase = !searchCase;
  applyToolbarState();
  partialUpdate({ remeasure: false });
});
searchInput.addEventListener('input', () => {
  searchQuery = searchInput.value;
  partialUpdate({ remeasure: false });
});
searchInput.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    searchInput.value = '';
    searchQuery = '';
    partialUpdate({ remeasure: false });
  }
});
scroller.addEventListener('scroll', scheduleScrollRender);
window.addEventListener('resize', () => {
  if (!view.wordWrap) scheduleScrollRender();
});

function bumpFont(delta: number): void {
  const current =
    view.fontSize > 0
      ? view.fontSize
      : parseFloat(getComputedStyle(document.body).fontSize) || 13;
  setFont(clamp(current + delta, MIN_FONT, MAX_FONT));
}

function setFont(size: number): void {
  vscode.postMessage({ type: 'setState', state: { fontSize: size } });
}

// === Boot ===
vscode.postMessage({ type: 'ready' });
