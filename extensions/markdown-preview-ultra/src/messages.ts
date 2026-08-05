/**
 * Message protocol between the extension host and the preview webview.
 *
 * Markdown → HTML happens in the host (Rust/WASM engine). The webview is a
 * thin patch-applier plus browser-only renderers (KaTeX, mermaid,
 * highlight.js). Every webview→host message is shape-checked on arrival —
 * the webview is treated as untrusted even though we authored it.
 */

/** One step of the engine's patch script (see crates/markdown-engine). */
export type Patch =
  | { op: 'keep'; count: number }
  | { op: 'delete'; count: number }
  | { op: 'insert'; html: string[] }
  | { op: 'replace'; count: number; html: string[] };

/** A heading in the document's TOC tree. `line` is 0-based. */
export interface TocEntry {
  level: number;
  text: string;
  slug: string;
  line: number;
  children: TocEntry[];
}

/** Front matter extracted by the engine. */
export interface Frontmatter {
  raw: string;
  data: unknown;
}

export type PreviewTheme = 'auto' | 'github-light' | 'github-dark';
export type MermaidTheme = 'auto' | 'default' | 'dark' | 'forest' | 'neutral';

/** Presentation settings forwarded to the webview. */
export interface PreviewSettings {
  scrollSync: boolean;
  math: boolean;
  mermaid: boolean;
  mermaidTheme: MermaidTheme;
  frontmatterDisplay: 'card' | 'hidden';
  theme: PreviewTheme;
  tocVisible: boolean;
  /** Default sidebar width in px; a dragged width overrides it per preview. */
  tocWidth: number;
  /** Task-checkbox toggling from the preview (default off). */
  taskToggle: boolean;
}

// ── Host → Webview ───────────────────────────────────────────────────

/** A render: patch script plus document-level extractions. */
export interface UpdateMessage {
  type: 'update';
  /** Monotonic; the webview ignores non-increasing sequence numbers. */
  seq: number;
  /** `true` → clear existing blocks before applying patches. */
  reset: boolean;
  patches: Patch[];
  toc: TocEntry[];
  frontmatter: Frontmatter | null;
  /** Source document URI (persisted by the webview for panel restore). */
  uri: string;
  /** Webview-resource base URI (with trailing slash) for relative resources. */
  baseHref: string;
  /** Webview URIs of user customCss files, applied in order. */
  customStyles: string[];
  settings: PreviewSettings;
  /** In-page light/dark switch, or `null` while the configured theme is in force. */
  themeOverride: PreviewTheme | null;
  /** Preview-local link history (drives the toolbar's ← / → buttons). */
  canGoBack: boolean;
  canGoForward: boolean;
}

/** Editor scrolled: align the preview so 0-based `line` sits at `ratio` of the viewport. */
export interface ScrollMessage {
  type: 'scroll';
  line: number;
  ratio: number;
}

/** Color theme kind changed; restyle mermaid/highlight without a re-render. */
export interface ThemeMessage {
  type: 'theme';
  kind: 'light' | 'dark';
}

/**
 * The panel became the on-screen tab in its group, or stopped being it.
 * `retainContextWhenHidden` keeps the webview's DOM alive while it is hidden
 * but not its layout, so the page stops measuring offsets until it is back.
 */
export interface VisibilityMessage {
  type: 'visibility';
  visible: boolean;
}

/**
 * The in-page light/dark switch was flipped — here or in another preview. The
 * host holds it for the whole window, so every open page follows along.
 */
export interface ThemeOverrideMessage {
  type: 'themeOverride';
  /** `null` → no override; the configured theme applies. */
  theme: PreviewTheme | null;
}

/** The WASM engine is not built; show a friendly hint instead of content. */
export interface NoEngineMessage {
  type: 'noEngine';
}

export type HostToWebview =
  | UpdateMessage
  | ScrollMessage
  | ThemeMessage
  | ThemeOverrideMessage
  | VisibilityMessage
  | NoEngineMessage;

// ── Webview → Host ───────────────────────────────────────────────────

/** Webview finished loading and is ready to receive an `update`. */
export interface ReadyMessage {
  type: 'ready';
}

/** Preview was scrolled by the user; reveal this 0-based line in the editor. */
export interface RevealLineMessage {
  type: 'revealLine';
  line: number;
}

/** Double-click / TOC navigation: reveal and focus the editor at this line. */
export interface JumpToLineMessage {
  type: 'jumpToLine';
  line: number;
}

/** Preview-local history navigation (the toolbar's ← / → buttons). */
export interface NavigateMessage {
  type: 'navigate';
  direction: 'back' | 'forward';
}

/** A link inside the rendered preview was clicked. */
export interface OpenLinkMessage {
  type: 'openLink';
  href: string;
}

/**
 * The in-page light/dark switch was flipped. The host records it for the window
 * and echoes back what it made of it; the configuration is never written.
 */
export interface SetThemeMessage {
  type: 'setTheme';
  theme: PreviewTheme;
}

/** Task checkbox clicked (only when `taskLists.toggleFromPreview` is on). */
export interface ToggleTaskMessage {
  type: 'toggleTask';
  line: number;
  checked: boolean;
}

/** Webview-side failure (e.g. patch applier); host logs and forces a reset. */
export interface ErrorMessage {
  type: 'error';
  message: string;
  context: string;
}

export type WebviewToHost =
  | ReadyMessage
  | RevealLineMessage
  | JumpToLineMessage
  | NavigateMessage
  | OpenLinkMessage
  | SetThemeMessage
  | ToggleTaskMessage
  | ErrorMessage;

// ── Validation ───────────────────────────────────────────────────────

function isFiniteNumber(v: unknown): v is number {
  return typeof v === 'number' && Number.isFinite(v);
}

function isPreviewTheme(v: unknown): v is PreviewTheme {
  return v === 'auto' || v === 'github-light' || v === 'github-dark';
}

/**
 * Shape-check an incoming webview message. No `any` dispatch: unknown or
 * malformed messages are dropped by the caller.
 */
export function isWebviewToHost(msg: unknown): msg is WebviewToHost {
  if (typeof msg !== 'object' || msg === null) return false;
  const m = msg as Record<string, unknown>;
  switch (m.type) {
    case 'ready':
      return true;
    case 'revealLine':
    case 'jumpToLine':
      return isFiniteNumber(m.line) && m.line >= 0;
    case 'navigate':
      return m.direction === 'back' || m.direction === 'forward';
    case 'openLink':
      return typeof m.href === 'string';
    case 'setTheme':
      return isPreviewTheme(m.theme);
    case 'toggleTask':
      return (
        isFiniteNumber(m.line) && m.line >= 0 && typeof m.checked === 'boolean'
      );
    case 'error':
      return typeof m.message === 'string' && typeof m.context === 'string';
    default:
      return false;
  }
}
