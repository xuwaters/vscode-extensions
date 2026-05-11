import bodyHtml from './body.html';
import styles from './styles.css';
import type {
  FilterRule,
  FilterSet,
  HostToWebview,
  LineRecord,
  ParsedLines,
  ViewState,
  WebviewToHost,
} from '../src/types.js';

declare const acquireVsCodeApi: () => {
  postMessage(message: WebviewToHost): void;
  setState(state: unknown): void;
  getState(): unknown;
};

// Inject styles and body. Using a constructable stylesheet avoids tripping
// the webview's CSP `style-src` (no inline <style> element). The host's HTML
// shell is intentionally minimal — see editorProvider.getHtmlForWebview.
const sheet = new CSSStyleSheet();
sheet.replaceSync(styles);
document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
document.body.insertAdjacentHTML('afterbegin', bodyHtml);

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
const btnSets = byId<HTMLButtonElement>('btn-sets');
const setsMenu = byId<HTMLDivElement>('sets-menu');
const filterEditor = byId<HTMLDivElement>('filter-editor');
const btnAnsi = byId<HTMLButtonElement>('btn-ansi');
const btnWrap = byId<HTMLButtonElement>('btn-wrap');
const btnMode = byId<HTMLButtonElement>('btn-mode');
const btnRegex = byId<HTMLButtonElement>('btn-regex');
const btnCase = byId<HTMLButtonElement>('btn-case');
const btnFontUp = byId<HTMLButtonElement>('btn-font-up');
const btnFontDown = byId<HTMLButtonElement>('btn-font-down');
const btnFontReset = byId<HTMLButtonElement>('btn-font-reset');
const btnText = byId<HTMLButtonElement>('btn-text');
const btnEnd = byId<HTMLButtonElement>('btn-end');
const reloadBanner = byId<HTMLDivElement>('reload-banner');
const reloadBannerText = byId<HTMLSpanElement>('reload-banner-text');
const btnReload = byId<HTMLButtonElement>('btn-reload');
const btnReloadDismiss = byId<HTMLButtonElement>('btn-reload-dismiss');

function byId<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) throw new Error(`#${id} not found`);
  return el as T;
}

// === State ===
let lines: ParsedLines = { html: [], text: [] };
let filterMatches: number[] = [];
let rules: FilterRule[] = [];
let sets: FilterSet[] = [];
let activeSetNames: Set<string> = new Set();
let palette: string[] = [];
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

// === Streaming mode state ===
//
// In streaming mode the webview no longer owns the entire file; lines are
// fetched on-demand from the host as a window scrolls into view, and
// filter/search results stream in incrementally. `streamMode` is the
// switch the data accessors below check.
let streamMode = false;
let streamTotalLines = 0;
let streamFileSize = 0;
let indexProgressInfo = { scannedLines: 0, scannedBytes: 0, complete: false };
const streamLineCache = new Map<number, LineRecord>();
const streamFilterTags = new Map<number, number>(); // line → 1-based rule index
const streamSearchHits = new Set<number>();
let streamSearchTotal = 0;
let streamFilterTruncated = false;
let streamSearchTruncated = false;
let pendingWindowRanges = new Set<string>();
let streamWindowReqSeq = 0;
const STREAM_WINDOW_SIZE = 200;

// Per-line measured pixel heights for streaming + wrap mode (RFC §9.1
// option B / phase 7). For unmeasured lines we use `streamAvgHeight`
// which is a running mean refined as lines are rendered.
const streamLineHeights = new Map<number, number>();
let streamAvgHeight = 18;

// === Rendering ===

function htmlEscape(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function lineTagOf(idx: number): number {
  if (streamMode) return streamFilterTags.get(idx) ?? 0;
  return filterMatches[idx] ?? 0;
}

function lineTextOf(idx: number): string {
  if (streamMode) return streamLineCache.get(idx)?.text ?? '';
  return lines.text[idx] ?? '';
}

function lineHtmlOf(idx: number): string {
  if (streamMode) return streamLineCache.get(idx)?.html ?? '';
  return lines.html[idx] ?? '';
}

function totalLineCount(): number {
  return streamMode ? streamTotalLines : lines.text.length;
}

function lineBackground(idx: number): string | null {
  const m = lineTagOf(idx);
  if (!m) return null;
  const rule = rules[m - 1];
  if (!rule || rule.enabled === false) return null;
  return rule.color ?? null;
}

function lineMatchesAnyEnabledFilter(idx: number): boolean {
  const m = lineTagOf(idx);
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
  if (streamMode) {
    if (!searchQuery) return true;
    return streamSearchHits.has(idx);
  }
  if (!searchMatcher) return true;
  const text = lines.text[idx] ?? '';
  searchMatcher.lastIndex = 0;
  return searchMatcher.test(text);
}

function recomputeVisibleIndices(): void {
  const total = totalLineCount();
  const onlyMatching = view.filterMode === 'only-matching';
  const anyEnabledRule = rules.some((r) => r.enabled !== false);

  if (streamMode) {
    if (!onlyMatching && !searchQuery) {
      // Highlight mode + no search: all lines are visible. Skip allocating a
      // 10M-element array — represent the range implicitly via length only.
      visibleIndices = makeRangeArray(total);
      return;
    }
    // Build the visible set from sparse host-supplied hits.
    const set = new Set<number>();
    if (onlyMatching && anyEnabledRule) {
      streamFilterTags.forEach((tag, line) => {
        const rule = rules[tag - 1];
        if (rule && rule.enabled !== false) set.add(line);
      });
    } else if (!onlyMatching) {
      for (let i = 0; i < total; i++) set.add(i);
    }
    if (searchQuery) {
      const intersect = new Set<number>();
      streamSearchHits.forEach((line) => {
        if (set.has(line) || (!onlyMatching && !anyEnabledRule)) intersect.add(line);
      });
      visibleIndices = Array.from(intersect).sort((a, b) => a - b);
    } else {
      visibleIndices = Array.from(set).sort((a, b) => a - b);
    }
    return;
  }

  const out: number[] = [];
  for (let i = 0; i < total; i++) {
    if (!lineMatchesSearch(i)) continue;
    if (onlyMatching && anyEnabledRule && !lineMatchesAnyEnabledFilter(i)) {
      continue;
    }
    out.push(i);
  }
  visibleIndices = out;
}

/** Cheap "range" array used by streaming highlight mode. */
function makeRangeArray(n: number): number[] {
  // For huge totals we can't allocate a dense array; instead use a Proxy-like
  // wrapper that virtualises index lookups. Plain JS arrays are fine up to a
  // few million entries; past that, fall back to a virtual array.
  if (n < 5_000_000) {
    const out = new Array<number>(n);
    for (let i = 0; i < n; i++) out[i] = i;
    return out;
  }
  // Virtual array: only `.length` and indexed reads are supported; that's
  // what the rest of the code uses for visibleIndices.
  const handler: ProxyHandler<number[]> = {
    get(target, prop) {
      if (prop === 'length') return n;
      if (typeof prop === 'string' && /^\d+$/.test(prop)) {
        const i = Number(prop);
        return i < n ? i : undefined;
      }
      return Reflect.get(target, prop);
    },
  };
  return new Proxy([] as number[], handler);
}

function requestWindowForLine(lineIdx: number): void {
  if (!streamMode) return;
  // Round to STREAM_WINDOW_SIZE; coalesce nearby requests.
  const start = Math.max(0, Math.floor(lineIdx / STREAM_WINDOW_SIZE) * STREAM_WINDOW_SIZE);
  const end = Math.min(streamTotalLines, start + STREAM_WINDOW_SIZE);
  const key = `${start}:${end}`;
  if (pendingWindowRanges.has(key)) return;
  // Skip if we already have the entire window cached.
  let allCached = true;
  for (let i = start; i < end; i++) {
    if (!streamLineCache.has(i)) {
      allCached = false;
      break;
    }
  }
  if (allCached) return;
  pendingWindowRanges.add(key);
  vscode.postMessage({
    type: 'requestWindow',
    requestId: ++streamWindowReqSeq,
    start,
    end,
  });
}

function applyLineContent(div: HTMLDivElement, idx: number): void {
  // In stream mode the line may not have been fetched yet; show a placeholder
  // and trigger a window request. The host will respond and we re-render.
  if (streamMode && !streamLineCache.has(idx)) {
    div.innerHTML = '​';
    div.style.backgroundColor = '';
    div.style.borderLeftColor = 'transparent';
    div.dataset['i'] = String(idx);
    requestWindowForLine(idx);
    return;
  }
  const lineText = lineTextOf(idx);
  const lineHtml = lineHtmlOf(idx);
  const html = view.renderAnsi ? lineHtml : htmlEscape(lineText);
  if (searchMatcher && lineText.length > 0) {
    div.innerHTML = highlightSearchInHtml(html, lineText);
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
// Live mapping from lineIdx → element currently rendered for that line. Keyed
// by line index (not slot position) so that scrolling does not disturb DOM
// nodes for lines that remain in view. Without this, a mouse-drag selection
// would flash on every scroll frame as the browser re-resolves the selection
// range against re-created DOM.
const renderedLineEls = new Map<number, HTMLDivElement>();
// When content (filters/search/font/etc.) changes, every line must be
// re-applied even if it's still in view. Scrolls leave this false.
let renderDirty = true;

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

function clearRenderedLines(): void {
  for (const el of renderedLineEls.values()) {
    if (el.parentNode === viewport) viewport.removeChild(el);
    recycleLineEl(el);
  }
  renderedLineEls.clear();
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

  if (renderDirty) {
    clearRenderedLines();
    renderDirty = false;
  }

  // Desired set: lineIdx → visIdx (for transform positioning).
  const desired = new Map<number, number>();
  for (let v = startVisIdx; v < endVisIdx; v++) {
    desired.set(visibleIndices[v], v);
  }

  // Drop elements that scrolled out of view.
  for (const [lineIdx, el] of Array.from(renderedLineEls)) {
    if (!desired.has(lineIdx)) {
      viewport.removeChild(el);
      recycleLineEl(el);
      renderedLineEls.delete(lineIdx);
    }
  }

  // Add or reposition.
  for (const [lineIdx, visIdx] of desired) {
    let el = renderedLineEls.get(lineIdx);
    const transform = `translateY(${visIdx * lineHeight}px)`;
    if (!el) {
      el = getLineEl();
      el.style.position = 'absolute';
      el.style.left = '0';
      el.style.right = '0';
      el.style.transform = transform;
      applyLineContent(el, lineIdx);
      viewport.appendChild(el);
      renderedLineEls.set(lineIdx, el);
    } else if (el.style.transform !== transform) {
      el.style.transform = transform;
    }
  }
}

function renderWrap(): void {
  if (streamMode) {
    renderWrapStreaming();
    return;
  }
  // In-memory wrap mode: variable line heights, virtualization disabled.
  // Render all visible lines flow-positioned. For very large logs this is
  // sluggish; wrap mode is opt-in.
  clearRenderedLines();
  spacer.style.height = '';
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

/**
 * Approximate wrap layout for streaming mode (RFC §9.1 option B):
 * render only the visible window flow-positioned at `firstVisible *
 * avgHeight`, where `avgHeight` is a running mean of measured line
 * heights. The scrollbar is approximate by design — jumps to unmeasured
 * regions land "close" and re-anchor once the window paints.
 */
function renderWrapStreaming(): void {
  spacer.style.height = `${visibleIndices.length * streamAvgHeight}px`;

  const scrollTop = scroller.scrollTop;
  const viewportHeight = scroller.clientHeight;
  const startVisIdx = Math.max(
    0,
    Math.floor(scrollTop / streamAvgHeight) - OVERSCAN,
  );
  const endVisIdx = Math.min(
    visibleIndices.length,
    Math.ceil((scrollTop + viewportHeight) / streamAvgHeight) + OVERSCAN,
  );

  clearRenderedLines();
  const frag = document.createDocumentFragment();
  const wrapper = document.createElement('div');
  wrapper.style.position = 'absolute';
  wrapper.style.left = '0';
  wrapper.style.right = '0';
  wrapper.style.transform = `translateY(${startVisIdx * streamAvgHeight}px)`;
  for (let v = startVisIdx; v < endVisIdx; v++) {
    const lineIdx = visibleIndices[v];
    const el = document.createElement('div');
    el.className = 'ln';
    el.style.position = 'static';
    applyLineContent(el, lineIdx);
    wrapper.appendChild(el);
    renderedLineEls.set(lineIdx, el);
  }
  frag.appendChild(wrapper);
  viewport.replaceChildren(frag);

  // After layout, sample heights and refresh the rolling average.
  measureStreamHeights(wrapper);
}

function measureStreamHeights(container: HTMLElement): void {
  const children = container.children;
  let touched = false;
  let totalSampled = 0;
  let sumSampled = 0;
  for (let i = 0; i < children.length; i++) {
    const el = children[i] as HTMLElement;
    const lineIdx = Number(el.dataset['i']);
    if (!Number.isFinite(lineIdx)) continue;
    const h = el.getBoundingClientRect().height;
    if (h <= 0) continue;
    streamLineHeights.set(lineIdx, h);
    totalSampled += 1;
    sumSampled += h;
    touched = true;
  }
  if (!touched) return;
  // Exponential moving average across all rendered windows. The weight
  // is proportional to how much of the file we've now measured, capped
  // so that we never get fully stuck on early samples.
  const newAvg = sumSampled / totalSampled;
  const measured = streamLineHeights.size;
  const weight = Math.min(0.5, measured / Math.max(1, streamTotalLines));
  const next = streamAvgHeight * (1 - weight) + newAvg * weight;
  if (Math.abs(next - streamAvgHeight) > 0.5) {
    streamAvgHeight = next;
    // Refresh spacer height so the scrollbar stays roughly accurate.
    spacer.style.height = `${visibleIndices.length * streamAvgHeight}px`;
  } else {
    streamAvgHeight = next;
  }
  void totalSampled;
}

function render(): void {
  renderDirty = true;
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
  const total = totalLineCount();
  const visible = visibleIndices.length;
  const bytes = streamMode ? streamFileSize : totalBytes;
  info.textContent = `${visible.toLocaleString()} / ${total.toLocaleString()} lines · ${fmtBytes(bytes)}`;
  if (streamMode) {
    if (!indexProgressInfo.complete) {
      banner.hidden = false;
      const pct = streamFileSize > 0
        ? Math.floor((indexProgressInfo.scannedBytes / streamFileSize) * 100)
        : 0;
      banner.textContent = `Indexing… ${pct}% (${indexProgressInfo.scannedLines.toLocaleString()} lines)`;
    } else if (streamFilterTruncated || streamSearchTruncated) {
      banner.hidden = false;
      banner.textContent = `Result limit reached; showing first results only.`;
    } else {
      banner.hidden = true;
    }
  } else {
    banner.hidden = !truncated;
    if (truncated) {
      banner.textContent =
        'File exceeds logViewer.maxFileSizeBytes; only the head is shown. Open in Text Editor to see the full file.';
    }
  }
  if (searchQuery) {
    let hits: number;
    if (streamMode) {
      hits = streamSearchHits.size;
    } else if (searchMatcher) {
      hits = 0;
      for (let i = 0; i < lines.text.length; i++) {
        searchMatcher.lastIndex = 0;
        if (searchMatcher.test(lines.text[i] ?? '')) hits++;
      }
    } else {
      hits = 0;
    }
    const suffix = streamMode && streamSearchTotal && streamSearchTotal !== hits
      ? ` (of ${streamSearchTotal.toLocaleString()})`
      : '';
    searchInfo.textContent = `${hits.toLocaleString()} match${hits === 1 ? '' : 'es'}${suffix}`;
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
      streamMode = false;
      lines = msg.lines;
      filterMatches = msg.filterMatches;
      rules = msg.rules;
      sets = msg.sets;
      activeSetNames = new Set(msg.activeSetNames);
      palette = msg.palette;
      view = msg.state;
      truncated = msg.truncated;
      totalBytes = msg.totalBytes;
      renderChips();
      renderSetsMenu();
      applyAll();
      break;
    case 'streamInit':
      streamMode = true;
      streamTotalLines = msg.totalLines;
      streamFileSize = msg.fileSize;
      indexProgressInfo = {
        scannedLines: msg.indexProgress.scannedLines,
        scannedBytes: msg.indexProgress.scannedBytes,
        complete: msg.indexProgress.complete,
      };
      rules = msg.rules;
      sets = msg.sets;
      activeSetNames = new Set(msg.activeSetNames);
      palette = msg.palette;
      view = msg.state;
      truncated = false;
      btnEnd.hidden = false;
      streamLineCache.clear();
      streamFilterTags.clear();
      streamSearchHits.clear();
      streamSearchTotal = 0;
      streamFilterTruncated = false;
      streamSearchTruncated = false;
      pendingWindowRanges = new Set();
      renderChips();
      renderSetsMenu();
      applyAll();
      break;
    case 'indexProgress':
      indexProgressInfo = {
        scannedLines: msg.progress.scannedLines,
        scannedBytes: msg.progress.scannedBytes,
        complete: msg.progress.complete,
      };
      if (msg.progress.totalLines !== undefined) {
        streamTotalLines = msg.progress.totalLines;
      }
      // Once index reaches further than what we've visualised, refresh.
      partialUpdate({ remeasure: false, preserveScroll: true });
      break;
    case 'window': {
      pendingWindowRanges.delete(`${msg.start}:${msg.start + msg.lines.length + (msg.partialCount ?? 0)}`);
      for (let i = 0; i < msg.lines.length; i++) {
        streamLineCache.set(msg.start + i, msg.lines[i]);
      }
      // Re-render in place; the spacer height doesn't change.
      renderDirty = true;
      if (view.wordWrap) renderWrap();
      else renderVirtualized();
      break;
    }
    case 'filterProgress':
      for (const [line, rule] of msg.hits) {
        streamFilterTags.set(line, rule + 1);
      }
      // Lightly re-render: tags affect background colour for already-visible
      // lines, and (in only-matching mode) affect visibility.
      if (view.filterMode === 'only-matching') {
        partialUpdate({ remeasure: false, preserveScroll: true });
      } else {
        renderDirty = true;
        if (view.wordWrap) renderWrap();
        else renderVirtualized();
        updateInfo();
      }
      break;
    case 'filterDone':
      streamFilterTruncated = msg.truncated;
      void msg.totalHits;
      partialUpdate({ remeasure: false, preserveScroll: true });
      break;
    case 'searchProgress':
      for (const line of msg.hits) streamSearchHits.add(line);
      partialUpdate({ remeasure: false, preserveScroll: true });
      break;
    case 'searchDone':
      streamSearchTotal = msg.totalHits;
      streamSearchTruncated = msg.truncated;
      partialUpdate({ remeasure: false, preserveScroll: true });
      break;
    case 'fileChanged':
      showReloadBanner(msg.previousSize, msg.currentSize);
      break;
    case 'update':
      if (msg.lines) lines = msg.lines;
      if (msg.filterMatches) filterMatches = msg.filterMatches;
      if (msg.rules) {
        rules = msg.rules;
        renderChips();
      }
      if (msg.sets) {
        sets = msg.sets;
        renderSetsMenu();
      }
      if (msg.activeSetNames) {
        activeSetNames = new Set(msg.activeSetNames);
        renderSetsMenu();
      }
      if (msg.palette) palette = msg.palette;
      if (msg.state) view = msg.state;
      if (msg.truncated !== undefined) truncated = msg.truncated;
      if (msg.totalBytes !== undefined) totalBytes = msg.totalBytes;
      partialUpdate({ remeasure: !!msg.lines, preserveScroll: !msg.lines });
      break;
    case 'openFilterEditor':
      openFilterEditor();
      break;
    case 'filterConfigSaveResult':
      handleSaveResult(msg.ok, msg.error);
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
btnEnd.addEventListener('click', () => scrollToEnd());
btnReload.addEventListener('click', () => {
  hideReloadBanner();
  vscode.postMessage({ type: 'reload' });
});
btnReloadDismiss.addEventListener('click', hideReloadBanner);
window.addEventListener('keydown', (e) => {
  if (e.key === 'End' && (e.ctrlKey || e.metaKey)) {
    scrollToEnd();
    e.preventDefault();
  }
  if (e.key === 'Home' && (e.ctrlKey || e.metaKey)) {
    scroller.scrollTop = 0;
    e.preventDefault();
  }
});

function scrollToEnd(): void {
  // Jump to the bottom of the spacer. In stream mode this triggers a
  // window request for the tail (served from the pre-built tail buffer
  // if indexing is still running) — RFC §5.3.
  scroller.scrollTop = scroller.scrollHeight;
  scheduleScrollRender();
}

function showReloadBanner(prevSize: number, curSize: number): void {
  reloadBanner.hidden = false;
  const delta = curSize - prevSize;
  const sign = delta >= 0 ? '+' : '';
  reloadBannerText.textContent = `File changed on disk (${sign}${fmtBytes(delta)}).`;
}
function hideReloadBanner(): void {
  reloadBanner.hidden = true;
}
btnFontUp.addEventListener('click', () => bumpFont(1));
btnFontDown.addEventListener('click', () => bumpFont(-1));
btnFontReset.addEventListener('click', () => setFont(0));
btnRegex.addEventListener('click', () => {
  searchRegex = !searchRegex;
  applyToolbarState();
  scheduleStreamSearch();
  partialUpdate({ remeasure: false });
});
btnCase.addEventListener('click', () => {
  searchCase = !searchCase;
  applyToolbarState();
  scheduleStreamSearch();
  partialUpdate({ remeasure: false });
});
searchInput.addEventListener('input', () => {
  searchQuery = searchInput.value;
  scheduleStreamSearch();
  partialUpdate({ remeasure: false });
});
searchInput.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    searchInput.value = '';
    searchQuery = '';
    if (streamMode) {
      streamSearchHits.clear();
      streamSearchTotal = 0;
      vscode.postMessage({ type: 'cancelSearch' });
    }
    partialUpdate({ remeasure: false });
  }
});

let streamSearchTimer: ReturnType<typeof setTimeout> | null = null;
function scheduleStreamSearch(): void {
  if (!streamMode) return;
  if (streamSearchTimer !== null) clearTimeout(streamSearchTimer);
  streamSearchTimer = setTimeout(() => {
    streamSearchTimer = null;
    streamSearchHits.clear();
    streamSearchTotal = 0;
    streamSearchTruncated = false;
    if (searchQuery) {
      vscode.postMessage({
        type: 'setSearch',
        query: searchQuery,
        regex: searchRegex,
        caseSensitive: searchCase,
      });
    } else {
      vscode.postMessage({ type: 'cancelSearch' });
    }
  }, 200);
}
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

// === Set picker dropdown ===

function renderSetsMenu(): void {
  setsMenu.replaceChildren();
  if (sets.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'dropdown-item';
    empty.style.opacity = '0.7';
    empty.textContent = 'No filter sets configured';
    setsMenu.appendChild(empty);
  } else {
    sets.forEach((s) => {
      const row = document.createElement('label');
      row.className = 'dropdown-item';
      const cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.checked = activeSetNames.has(s.name);
      cb.addEventListener('change', () => {
        if (cb.checked) activeSetNames.add(s.name);
        else activeSetNames.delete(s.name);
        vscode.postMessage({
          type: 'setActiveSets',
          names: Array.from(activeSetNames),
        });
      });
      const label = document.createElement('span');
      label.textContent = s.name;
      label.style.flex = '1 1 auto';
      row.appendChild(cb);
      row.appendChild(label);
      const count = document.createElement('span');
      count.style.opacity = '0.6';
      count.textContent = `${s.filters.length}`;
      row.appendChild(count);
      setsMenu.appendChild(row);
    });
  }
  const sep = document.createElement('div');
  sep.className = 'dropdown-sep';
  setsMenu.appendChild(sep);
  const edit = document.createElement('div');
  edit.className = 'dropdown-item action';
  edit.textContent = 'Edit filter sets…';
  edit.addEventListener('click', () => {
    closeSetsMenu();
    openFilterEditor();
  });
  setsMenu.appendChild(edit);
  btnSets.classList.toggle('active', activeSetNames.size > 0);
}

function toggleSetsMenu(): void {
  if (setsMenu.hidden) {
    setsMenu.hidden = false;
    setTimeout(() => document.addEventListener('mousedown', onDocMouseDown), 0);
  } else {
    closeSetsMenu();
  }
}

function closeSetsMenu(): void {
  setsMenu.hidden = true;
  document.removeEventListener('mousedown', onDocMouseDown);
}

function onDocMouseDown(e: MouseEvent): void {
  const target = e.target as Node;
  if (setsMenu.contains(target) || btnSets.contains(target)) return;
  closeSetsMenu();
}

btnSets.addEventListener('click', (e) => {
  e.stopPropagation();
  toggleSetsMenu();
});

// === Filter editor modal ===

const DEFAULT_NEW_COLOR = '#5a1f1f';

interface EditorState {
  sets: FilterSet[];
  palette: string[];
  selectedSet: number;
  openColorRule: { setIdx: number; ruleIdx: number } | null;
  saving: boolean;
  error: string | null;
}

let editorState: EditorState | null = null;

function cloneSets(s: FilterSet[]): FilterSet[] {
  return s.map((set) => ({
    name: set.name,
    description: set.description,
    enabled: set.enabled,
    filters: set.filters.map((r) => ({ ...r })),
  }));
}

function openFilterEditor(): void {
  editorState = {
    sets: cloneSets(sets),
    palette: palette.slice(),
    selectedSet: sets.length > 0 ? 0 : -1,
    openColorRule: null,
    saving: false,
    error: null,
  };
  filterEditor.hidden = false;
  renderEditor();
}

function closeFilterEditor(): void {
  filterEditor.hidden = true;
  filterEditor.replaceChildren();
  editorState = null;
}

function renderEditor(): void {
  if (!editorState) return;
  const st = editorState;
  filterEditor.replaceChildren();

  const modal = el('div', 'modal');

  const header = el('div', 'modal-header');
  const h = document.createElement('h2');
  h.textContent = 'Filter Sets';
  header.appendChild(h);
  const closeBtn = el('button', 'icon-btn');
  closeBtn.textContent = '✕';
  closeBtn.title = 'Close';
  closeBtn.addEventListener('click', closeFilterEditor);
  header.appendChild(closeBtn);
  modal.appendChild(header);

  const body = el('div', 'modal-body');
  const sidebar = el('div', 'modal-sidebar');
  st.sets.forEach((s, i) => {
    const item = el('div', 'set-item' + (i === st.selectedSet ? ' selected' : ''));
    const nm = el('span', 'set-name');
    nm.textContent = s.name || '(unnamed)';
    item.appendChild(nm);
    const rm = el('button', 'icon-btn');
    rm.textContent = '🗑';
    rm.title = 'Delete set';
    rm.addEventListener('click', (e) => {
      e.stopPropagation();
      st.sets.splice(i, 1);
      if (st.selectedSet >= st.sets.length) st.selectedSet = st.sets.length - 1;
      renderEditor();
    });
    item.appendChild(rm);
    item.addEventListener('click', () => {
      st.selectedSet = i;
      st.openColorRule = null;
      renderEditor();
    });
    sidebar.appendChild(item);
  });
  const addSet = el('div', 'set-item');
  addSet.style.color = 'var(--vscode-textLink-foreground)';
  addSet.textContent = '+ New set';
  addSet.addEventListener('click', () => {
    st.sets.push({
      name: `Set ${st.sets.length + 1}`,
      enabled: true,
      filters: [],
    });
    st.selectedSet = st.sets.length - 1;
    renderEditor();
  });
  sidebar.appendChild(addSet);
  body.appendChild(sidebar);

  const main = el('div', 'modal-main');
  const current = st.sets[st.selectedSet];
  if (!current) {
    const empty = el('div');
    empty.style.opacity = '0.6';
    empty.textContent = 'Select or create a filter set to edit its rules.';
    main.appendChild(empty);
  } else {
    main.appendChild(renderSetMeta(current));
    main.appendChild(renderRulesTable(current));
  }
  body.appendChild(main);
  modal.appendChild(body);

  const footer = el('div', 'modal-footer');
  if (st.error) {
    const err = el('span', 'save-error');
    err.textContent = st.error;
    footer.appendChild(err);
  }
  const cancel = el('button');
  cancel.textContent = 'Cancel';
  cancel.addEventListener('click', closeFilterEditor);
  footer.appendChild(cancel);
  const save = el<HTMLButtonElement>('button', 'primary');
  save.textContent = st.saving ? 'Saving…' : 'Save';
  save.disabled = st.saving;
  save.addEventListener('click', saveEditor);
  footer.appendChild(save);
  modal.appendChild(footer);

  filterEditor.appendChild(modal);
}

function renderSetMeta(set: FilterSet): HTMLElement {
  const wrap = el('div', 'set-meta');
  const nameField = field('Name');
  const nameInput = document.createElement('input');
  nameInput.type = 'text';
  nameInput.value = set.name;
  nameInput.addEventListener('input', () => {
    set.name = nameInput.value;
    // Update sidebar label without full re-render.
    const item = filterEditor.querySelector(
      `.set-item.selected .set-name`,
    ) as HTMLElement | null;
    if (item) item.textContent = nameInput.value || '(unnamed)';
  });
  nameField.appendChild(nameInput);
  wrap.appendChild(nameField);

  const descField = field('Description');
  const descInput = document.createElement('input');
  descInput.type = 'text';
  descInput.value = set.description ?? '';
  descInput.placeholder = 'When to use this set…';
  descInput.addEventListener('input', () => {
    set.description = descInput.value || undefined;
  });
  descField.appendChild(descInput);
  wrap.appendChild(descField);

  const enField = field('Enabled by default');
  const cb = document.createElement('input');
  cb.type = 'checkbox';
  cb.checked = set.enabled !== false;
  cb.addEventListener('change', () => {
    set.enabled = cb.checked;
  });
  enField.appendChild(cb);
  wrap.appendChild(enField);

  return wrap;
}

function renderRulesTable(set: FilterSet): HTMLElement {
  const wrap = el('div');
  const title = el('div');
  title.style.marginTop = '4px';
  title.style.fontWeight = '600';
  title.textContent = 'Rules';
  wrap.appendChild(title);

  const table = el('div', 'rules-table');
  for (const h of ['Name', 'Pattern', 'Regex', 'Case', 'Color', '']) {
    const c = el('div', 'head');
    c.textContent = h;
    table.appendChild(c);
  }
  set.filters.forEach((rule, i) => {
    appendRuleRow(table, set, rule, i);
  });
  wrap.appendChild(table);

  const add = el('button');
  add.textContent = '+ Add rule';
  add.style.marginTop = '6px';
  add.addEventListener('click', () => {
    set.filters.push({
      name: 'New rule',
      pattern: '',
      regex: false,
      caseSensitive: false,
      color: pickInitialColor(),
      enabled: true,
    });
    renderEditor();
  });
  wrap.appendChild(add);
  return wrap;
}

function appendRuleRow(
  table: HTMLElement,
  set: FilterSet,
  rule: FilterRule,
  ruleIdx: number,
): void {
  const name = document.createElement('input');
  name.type = 'text';
  name.value = rule.name;
  name.addEventListener('input', () => {
    rule.name = name.value;
  });
  table.appendChild(name);

  const pat = document.createElement('input');
  pat.type = 'text';
  pat.value = rule.pattern;
  pat.placeholder = 'substring or regex';
  pat.addEventListener('input', () => {
    rule.pattern = pat.value;
  });
  table.appendChild(pat);

  const rx = document.createElement('input');
  rx.type = 'checkbox';
  rx.checked = !!rule.regex;
  rx.title = 'Regex';
  rx.addEventListener('change', () => {
    rule.regex = rx.checked;
  });
  const rxWrap = el('div');
  rxWrap.style.textAlign = 'center';
  rxWrap.appendChild(rx);
  table.appendChild(rxWrap);

  const cs = document.createElement('input');
  cs.type = 'checkbox';
  cs.checked = !!rule.caseSensitive;
  cs.title = 'Case-sensitive';
  cs.addEventListener('change', () => {
    rule.caseSensitive = cs.checked;
  });
  const csWrap = el('div');
  csWrap.style.textAlign = 'center';
  csWrap.appendChild(cs);
  table.appendChild(csWrap);

  const colorCell = el('div', 'color-cell');
  const sw = el('span', 'swatch');
  sw.style.background = rule.color ?? 'transparent';
  colorCell.appendChild(sw);
  const colorLabel = el('span');
  colorLabel.textContent = rule.color ?? 'pick…';
  colorLabel.style.fontFamily = 'var(--vscode-editor-font-family, monospace)';
  colorLabel.style.fontSize = '11px';
  colorCell.appendChild(colorLabel);
  colorCell.addEventListener('click', (e) => {
    e.stopPropagation();
    const st = editorState;
    if (!st) return;
    const setIdx = st.selectedSet;
    const open = st.openColorRule;
    if (open && open.setIdx === setIdx && open.ruleIdx === ruleIdx) {
      st.openColorRule = null;
      renderEditor();
    } else {
      st.openColorRule = { setIdx, ruleIdx };
      renderEditor();
    }
  });
  if (
    editorState?.openColorRule &&
    editorState.openColorRule.setIdx === editorState.selectedSet &&
    editorState.openColorRule.ruleIdx === ruleIdx
  ) {
    colorCell.appendChild(renderColorPopover(rule));
  }
  table.appendChild(colorCell);

  const del = el('button', 'icon-btn');
  del.textContent = '✕';
  del.title = 'Delete rule';
  del.addEventListener('click', () => {
    set.filters.splice(ruleIdx, 1);
    if (editorState?.openColorRule?.ruleIdx === ruleIdx) {
      editorState.openColorRule = null;
    }
    renderEditor();
  });
  table.appendChild(del);
}

function renderColorPopover(rule: FilterRule): HTMLElement {
  const pop = el('div', 'color-popover');
  pop.addEventListener('click', (e) => e.stopPropagation());
  const st = editorState!;
  st.palette.forEach((c) => {
    const cell = el('div', 'pal' + (eqColor(c, rule.color) ? ' selected' : ''));
    cell.style.background = c;
    cell.title = c;
    cell.addEventListener('click', () => {
      rule.color = c;
      st.openColorRule = null;
      renderEditor();
    });
    pop.appendChild(cell);
  });

  const add = el('div', 'add-color');
  const colorInput = document.createElement('input');
  colorInput.type = 'color';
  colorInput.value = sanitizeHex(rule.color) ?? DEFAULT_NEW_COLOR;
  const hexInput = document.createElement('input');
  hexInput.type = 'text';
  hexInput.placeholder = '#rrggbb / rgba(...)';
  hexInput.value = rule.color ?? '';
  colorInput.addEventListener('input', () => {
    hexInput.value = colorInput.value;
  });
  const addBtn = el('button');
  addBtn.textContent = 'Add';
  addBtn.title = 'Add to palette and apply';
  addBtn.addEventListener('click', () => {
    const c = hexInput.value.trim() || colorInput.value;
    if (!c) return;
    if (!st.palette.some((p) => eqColor(p, c))) st.palette.push(c);
    rule.color = c;
    st.openColorRule = null;
    renderEditor();
  });
  add.appendChild(colorInput);
  add.appendChild(hexInput);
  add.appendChild(addBtn);
  pop.appendChild(add);
  return pop;
}

function eqColor(a: string | undefined, b: string | undefined): boolean {
  if (!a || !b) return false;
  return a.trim().toLowerCase() === b.trim().toLowerCase();
}

function sanitizeHex(c: string | undefined): string | null {
  if (!c) return null;
  const m = c.trim().match(/^#([0-9a-fA-F]{6})$/);
  return m ? `#${m[1]}` : null;
}

function pickInitialColor(): string {
  const used = new Set(
    editorState?.sets.flatMap((s) => s.filters.map((r) => r.color ?? '')) ?? [],
  );
  const pool = editorState?.palette ?? [];
  for (const c of pool) if (!used.has(c)) return c;
  return pool[0] ?? DEFAULT_NEW_COLOR;
}

function saveEditor(): void {
  if (!editorState) return;
  editorState.error = null;
  editorState.saving = true;
  renderEditor();
  // Trim empty names; validate regexes.
  const cleaned: FilterSet[] = editorState.sets
    .map((s) => ({
      name: (s.name ?? '').trim() || 'Unnamed',
      description: s.description,
      enabled: s.enabled !== false,
      filters: s.filters
        .filter((r) => (r.name ?? '').trim() && (r.pattern ?? '').length > 0)
        .map((r) => ({
          name: r.name.trim(),
          pattern: r.pattern,
          regex: !!r.regex,
          caseSensitive: !!r.caseSensitive,
          color: r.color,
          enabled: r.enabled !== false,
        })),
    }));
  for (const s of cleaned) {
    for (const r of s.filters) {
      if (r.regex) {
        try {
          new RegExp(r.pattern);
        } catch (e) {
          editorState.error = `Invalid regex in "${s.name} / ${r.name}": ${(e as Error).message}`;
          editorState.saving = false;
          renderEditor();
          return;
        }
      }
    }
  }
  vscode.postMessage({
    type: 'saveFilterConfig',
    sets: cleaned,
    palette: editorState.palette,
  });
}

function handleSaveResult(ok: boolean, error: string | undefined): void {
  if (!editorState) return;
  editorState.saving = false;
  if (ok) {
    closeFilterEditor();
  } else {
    editorState.error = error ?? 'Failed to save.';
    renderEditor();
  }
}

function el<T extends HTMLElement = HTMLDivElement>(
  tag = 'div',
  className?: string,
): T {
  const e = document.createElement(tag) as unknown as T;
  if (className) e.className = className;
  return e;
}

function field(label: string): HTMLElement {
  const f = el('div', 'field');
  const l = el('label');
  l.textContent = label;
  f.appendChild(l);
  return f;
}

// Allow Escape to close the editor modal.
window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && editorState) {
    closeFilterEditor();
    e.stopPropagation();
  }
});

// === Boot ===
vscode.postMessage({ type: 'ready' });
