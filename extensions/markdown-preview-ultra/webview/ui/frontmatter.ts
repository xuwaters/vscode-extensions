import type { Frontmatter } from '../../src/messages';

/** Render the front matter card from the engine's structured data. */
export function renderFrontmatter(
  container: HTMLElement,
  frontmatter: Frontmatter | null,
  display: 'card' | 'hidden',
): void {
  container.textContent = '';
  if (!frontmatter || display === 'hidden') return;

  const card = document.createElement('div');
  card.className = 'frontmatter-card';
  const label = document.createElement('div');
  label.className = 'frontmatter-label';
  label.textContent = 'Frontmatter';
  card.append(label);

  const data = frontmatter.data;
  if (data && typeof data === 'object' && !Array.isArray(data)) {
    const dl = document.createElement('dl');
    for (const [key, value] of Object.entries(data as Record<string, unknown>)) {
      const dt = document.createElement('dt');
      dt.textContent = key;
      const dd = document.createElement('dd');
      dd.textContent = formatValue(value);
      dl.append(dt, dd);
    }
    card.append(dl);
  } else {
    const pre = document.createElement('pre');
    pre.textContent = frontmatter.raw.trim();
    card.append(pre);
  }
  container.append(card);
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return '';
  if (Array.isArray(value)) return value.map((v) => formatValue(v)).join(', ');
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}
