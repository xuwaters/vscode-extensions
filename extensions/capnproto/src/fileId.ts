import { randomBytes } from 'node:crypto';

/**
 * A fresh file id in the form `capnp id` prints: 64 random bits with the top
 * bit set, which Cap'n Proto requires of ids so they never collide with the
 * small ones derived from parent scopes.
 */
export function generateFileId(random: (size: number) => Uint8Array = randomBytes): string {
  const bytes = random(8);
  bytes[0] |= 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
  return `@0x${hex}`;
}

export interface ExistingFileId {
  /** Offsets of the `@0x…` token, without the `;`. */
  start: number;
  end: number;
  id: string;
}

/** The file id statement at the start of a line, if the file has one. */
export function findFileId(text: string): ExistingFileId | null {
  const m = /^[ \t]*(@0[xX][0-9a-fA-F]+)[ \t]*;/m.exec(text);
  if (!m) return null;
  const start = m.index + m[0].indexOf('@');
  return { start, end: start + m[1].length, id: m[1] };
}

export interface Insertion {
  offset: number;
  text: string;
}

/**
 * Where a new `@0x…;` line goes: after any leading comment block (licence or
 * file header), separated from what follows by one blank line.
 */
export function fileIdInsertion(text: string, id: string): Insertion {
  // End of the last line of the leading comment block (0 if there is none).
  let offset = 0;
  let pos = 0;
  while (pos < text.length) {
    const nl = text.indexOf('\n', pos);
    const lineEnd = nl === -1 ? text.length : nl + 1;
    const line = text.slice(pos, lineEnd).trim();
    if (line !== '' && !line.startsWith('#')) break;
    if (line !== '') offset = lineEnd;
    pos = lineEnd;
  }

  const before = text.slice(0, offset);
  const after = text.slice(offset);
  let insert = '';
  if (before !== '' && !before.endsWith('\n')) insert += '\n';
  if (before.trim() !== '') insert += '\n';
  insert += `${id};\n`;
  if (after.trim() !== '') insert += after.startsWith('\n') ? '' : '\n';
  return { offset, text: insert };
}
