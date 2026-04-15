/**
 * Simple syntax highlighting for code blocks using CSS classes.
 * A lightweight approach that works without heavy dependencies.
 * Can be replaced with Shiki or highlight.js in Phase 3.
 */

/** Apply basic keyword highlighting to a code block. Returns escaped HTML with span tags. */
export function highlightCode(code: string, lang: string): string {
  // For now, return escaped HTML with a language class.
  // Phase 3 will integrate a proper highlighter (Shiki/highlight.js).
  return `<pre class="code-block"><code class="language-${escapeAttr(lang)}">${escapeHtml(code)}</code></pre>`;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function escapeAttr(text: string): string {
  return text.replace(/[^a-zA-Z0-9_-]/g, '');
}
