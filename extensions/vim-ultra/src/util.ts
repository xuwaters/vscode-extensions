/** Pure helpers, kept free of the vscode module so vitest can load them. */

/**
 * Split a `type`-command payload into engine keys: one per Unicode code
 * point, so astral chars (emoji) arrive as a single key.
 */
export function typedKeys(text: string): string[] {
  return Array.from(text);
}

/** Rewrite engine-emitted `\n` line breaks to the document's EOL. */
export function replaceEol(text: string, eol: string): string {
  return eol === '\n' ? text : text.split('\n').join(eol);
}

interface PosLike {
  line: number;
  character: number;
}

interface SelectionLike {
  anchor: PosLike;
  active: PosLike;
}

/** Stable serialization for change-suppression comparisons. */
export function serializeSelections(sels: readonly SelectionLike[]): string {
  return sels
    .map(
      (s) => `${s.anchor.line}:${s.anchor.character}-${s.active.line}:${s.active.character}`,
    )
    .join(',');
}

/**
 * Status bar label for an engine mode, its pending keys and last message,
 * following vim-vscode's design: an engine report ("match 11 of 11",
 * ":s" counts, errors) replaces the whole label until the next key, and an
 * open command line ("/pat", "?pat", ":cmd") replaces the mode text, shown
 * with a `|` cursor. More than one cursor says so, since every key runs at
 * all of them.
 */
export function modeLabel(
  mode: string,
  pending: string,
  message = '',
  cursors = 1,
): string {
  if (message) return message;
  if (/^[/?:]/.test(pending)) return `${pending}|`;
  const base =
    mode === 'insert'
      ? '-- INSERT --'
      : mode === 'visual'
        ? '-- VISUAL --'
        : mode === 'visualLine'
          ? '-- VISUAL LINE --'
          : '-- NORMAL --';
  const multi = cursors > 1 ? `(${cursors} cursors)` : '';
  return [base, pending, multi].filter(Boolean).join(' ');
}
