/** Render an MDX component block as a styled placeholder card. */
export function renderMdxComponentHtml(raw: string): string {
  const lines = raw.split('\n');
  const firstLine = lines[0]?.trim() ?? '';

  // Detect if it's an import/export or a JSX component
  if (/^(import|export)\s/.test(firstLine)) {
    return `<div class="mdx-component-card mdx-import"><code>${escapeHtml(raw)}</code></div>`;
  }

  // JSX component — extract tag name
  const tagMatch = firstLine.match(/^<(\w+)/);
  const tagName = tagMatch ? tagMatch[1] : 'Component';

  // Try to find children (content between opening and closing tags)
  const openTagEnd = raw.indexOf('>');
  const closeTagStart = raw.lastIndexOf('</');

  let childrenHtml = '';
  if (openTagEnd >= 0 && closeTagStart > openTagEnd) {
    const children = raw.slice(openTagEnd + 1, closeTagStart).trim();
    if (children) {
      childrenHtml = `<div class="mdx-children">${escapeHtml(children)}</div>`;
    }
  }

  return `<div class="mdx-component-card">
  <span class="mdx-tag">&lt;${escapeHtml(tagName)}&gt;</span>
  ${childrenHtml}
  <span class="mdx-tag">&lt;/${escapeHtml(tagName)}&gt;</span>
</div>`;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
