/**
 * Message protocol between the extension host and the preview webview.
 *
 * The host owns the source document; the webview is a read-only renderer.
 * On every relevant change the host ships the full markdown text and the
 * webview re-renders it (markdown-it + KaTeX + mermaid + highlight.js).
 */

/** Render-time options forwarded to the webview, derived from configuration. */
export interface PreviewSettings {
  /** Render `$inline$` / `$$display$$` LaTeX via KaTeX. */
  math: boolean;
  /** Render ```mermaid fences as SVG diagrams. */
  mermaid: boolean;
  /** Render leading YAML frontmatter as a metadata card. */
  frontmatter: boolean;
  /** Convert a single newline inside a paragraph into a `<br>`. */
  breaks: boolean;
  /** Autoconvert bare URLs into links. */
  linkify: boolean;
  /** Keep the preview scrolled to the editor's top visible line. */
  scrollSync: boolean;
}

// ── Host → Webview ───────────────────────────────────────────────────

/** Full document content; the webview re-renders from scratch. */
export interface UpdateMessage {
  type: 'update';
  markdown: string;
  /** Display name shown in errors/title (basename of the source file). */
  fileName: string;
  /** Webview-resource base URI (with trailing slash) for relative images. */
  baseHref: string;
  settings: PreviewSettings;
}

/** Editor scrolled/moved; align the preview to this 0-based source line. */
export interface ScrollMessage {
  type: 'scroll';
  line: number;
}

export type HostToWebview = UpdateMessage | ScrollMessage;

// ── Webview → Host ───────────────────────────────────────────────────

/** Webview finished loading and is ready to receive an `update`. */
export interface ReadyMessage {
  type: 'ready';
}

/** A link inside the rendered preview was clicked. */
export interface OpenLinkMessage {
  type: 'openLink';
  href: string;
}

export type WebviewToHost = ReadyMessage | OpenLinkMessage;
