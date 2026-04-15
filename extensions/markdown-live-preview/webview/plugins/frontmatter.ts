/** Parse YAML frontmatter into key-value pairs (simple flat parsing). */
export function parseFrontmatter(raw: string): Record<string, string> {
  const result: Record<string, string> = {};
  const lines = raw.split('\n').filter((l) => l.trim() !== '---' && l.trim() !== '');

  for (const line of lines) {
    const match = line.match(/^(\w[\w\s]*?):\s*(.*)$/);
    if (match) {
      result[match[1].trim()] = match[2].trim();
    }
  }

  return result;
}

/** Render frontmatter as an HTML card. */
export function renderFrontmatterHtml(data: Record<string, string>): string {
  const entries = Object.entries(data);
  if (entries.length === 0) return '';

  const dl = entries
    .map(
      ([k, v]) =>
        `<dt>${escapeHtml(k)}</dt><dd>${escapeHtml(v)}</dd>`,
    )
    .join('');

  return `<div class="frontmatter-card"><div class="frontmatter-label">Frontmatter</div><dl>${dl}</dl></div>`;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
