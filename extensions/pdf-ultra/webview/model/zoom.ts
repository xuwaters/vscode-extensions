/**
 * Read a zoom percentage the reader typed into the toolbar.
 *
 * Forgiving about how it is written — `150`, `150%`, ` 150 % ` and `1.5x` all
 * mean the same thing — and strict about what it accepts, because a value that
 * cannot be read has to leave the current zoom alone rather than snap the page
 * to some fallback. Returns a percentage, or null when the text is not one.
 *
 * The range is not enforced here: `clampZoom` does that, and clamping in one
 * place keeps the buttons, the keyboard shortcuts and this box in agreement.
 */
export function parseZoomPercent(text: string): number | null {
  const trimmed = text.trim();

  // A factor (`1.5x`) is a percentage times a hundred; anything else is already
  // a percentage, with the sign an optional decoration.
  const asFactor = /^(\d+(?:\.\d+)?|\.\d+)\s*x$/i.exec(trimmed);
  const asPercent = /^(\d+(?:\.\d+)?|\.\d+)\s*%?$/.exec(trimmed);

  const match = asFactor ?? asPercent;
  if (!match) return null;

  const value = Number(match[1]) * (asFactor ? 100 : 1);
  if (!Number.isFinite(value) || value <= 0) return null;
  return value;
}

/** How a zoom factor is written back into the box. */
export function formatZoomPercent(zoom: number): string {
  return `${Math.round(zoom * 100)}%`;
}
