// Syntax highlighting for fenced code blocks. We use highlight.js's "common"
// bundle (≈40 popular languages) to keep the webview bundle reasonable while
// still covering the languages most markdown authors reach for.
import hljs from 'highlight.js/lib/common';

/** Highlight `code` for `lang`, returning HTML. Falls back to escaped text. */
export function highlightToHtml(code: string, lang: string): string {
  const language = lang.toLowerCase();
  if (language && hljs.getLanguage(language)) {
    try {
      return hljs.highlight(code, { language, ignoreIllegals: true }).value;
    } catch {
      // Unsupported syntax for the grammar — fall through to plain escaping.
    }
  }
  return escapeHtml(code);
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}
