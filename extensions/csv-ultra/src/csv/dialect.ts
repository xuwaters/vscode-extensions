/**
 * What "separated values" means for one file.
 *
 * Pure, and shared by both sides of the webview boundary: the extension host
 * writes cells back through it, and the page parses the same bytes with it. A
 * dialect the two disagreed about would be a table that edits into a different
 * file than the one on screen, so there is exactly one of these per open
 * document and it travels with every message that carries text.
 */
export interface Dialect {
  /** One character. Never a quote, a carriage return, or a line feed. */
  readonly delimiter: string;
  /** The character that protects a field containing the delimiter. */
  readonly quote: string;
  /** What separates records when this file is written back. */
  readonly newline: '\n' | '\r\n';
}

/** The delimiters `auto` scores a file against, best-known first. */
export const DELIMITER_CANDIDATES = [',', '\t', ';', '|', ':'] as const;

/** How a delimiter is spelled in the UI, since four of the five are punctuation. */
export const DELIMITER_NAMES: ReadonlyArray<{ value: string; label: string }> = [
  { value: ',', label: 'Comma' },
  { value: '\t', label: 'Tab' },
  { value: ';', label: 'Semicolon' },
  { value: '|', label: 'Pipe' },
  { value: ':', label: 'Colon' },
];

/** The name of a delimiter, for a status bar or a quick pick. */
export function delimiterName(delimiter: string): string {
  return DELIMITER_NAMES.find((entry) => entry.value === delimiter)?.label ?? delimiter;
}

/** How many records of the head `sniffDelimiter` reads before deciding. */
const SNIFF_RECORDS = 40;

/** How much of a file's head is worth reading to find `SNIFF_RECORDS` of it. */
const SNIFF_CHARS = 1 << 16;

export const DEFAULT_DIALECT: Dialect = { delimiter: ',', quote: '"', newline: '\n' };

/**
 * The delimiter a file's *name* settles, if any.
 *
 * A `.tsv` is tab-separated even when its first forty records happen to hold
 * more semicolons than tabs, so the extension outranks the content — this is
 * the one signal about a file that is not a guess.
 */
export function delimiterForPath(path: string): string | undefined {
  const dot = path.lastIndexOf('.');
  if (dot < 0) return undefined;
  switch (path.slice(dot + 1).toLowerCase()) {
    case 'tsv':
    case 'tab':
      return '\t';
    case 'psv':
      return '|';
    case 'csv':
      return ',';
    default:
      return undefined;
  }
}

/**
 * Which line ending a file already uses.
 *
 * The *first* one wins rather than the most common: a file with mixed endings
 * is a file two tools have written, and matching the one at the top keeps an
 * appended row looking like the rows above it. A file with no line break at all
 * has no opinion, so the caller's default stands.
 */
export function newlineOf(text: string, fallback: '\n' | '\r\n' = '\n'): '\n' | '\r\n' {
  const at = text.indexOf('\n');
  if (at < 0) return fallback;
  return at > 0 && text.charCodeAt(at - 1) === 13 ? '\r\n' : '\n';
}

/**
 * Guess the delimiter from the file's content.
 *
 * Each candidate is scored by splitting the head of the file *as if* that
 * candidate were the delimiter — quotes and all, so a comma inside `"Smith,
 * John"` never counts as a separator — and asking two questions of the result:
 * how many fields a record has, and how often every record agrees on it. A real
 * delimiter cuts a table into equal rows; an incidental character cuts it into
 * ragged ones.
 *
 * The score is `(fields - 1) × agreement²`, which is deliberately blunt:
 *
 * * `fields - 1` prefers the character that actually divides the file, and is
 *   zero for one that never appears — so a candidate absent from the sample can
 *   never win.
 * * `agreement²` — the fraction of records with the most common field count,
 *   squared — is what stops a stray character from beating a real one merely by
 *   being frequent. A semicolon scattered through prose is common and ragged;
 *   squaring makes raggedness expensive.
 *
 * Ties go to the earlier candidate, which is why `DELIMITER_CANDIDATES` is in
 * best-known-first order: a single-column file agrees perfectly with every
 * candidate at one field each, scores zero all round, and comes out a comma.
 */
export function sniffDelimiter(
  text: string,
  candidates: readonly string[] = DELIMITER_CANDIDATES,
): string {
  const truncated = text.length > SNIFF_CHARS;
  const sample = truncated ? text.slice(0, SNIFF_CHARS) : text;
  let best = candidates[0] ?? ',';
  let bestScore = 0;

  for (const delimiter of candidates) {
    const counts = fieldCounts(sample, delimiter, truncated);
    if (counts.length === 0) continue;

    const tally = new Map<number, number>();
    for (const count of counts) tally.set(count, (tally.get(count) ?? 0) + 1);

    let mode = 1;
    let modeCount = 0;
    for (const [fields, seen] of tally) {
      if (seen > modeCount || (seen === modeCount && fields > mode)) {
        mode = fields;
        modeCount = seen;
      }
    }

    const agreement = modeCount / counts.length;
    const score = (mode - 1) * agreement * agreement;
    if (score > bestScore) {
      bestScore = score;
      best = delimiter;
    }
  }

  return best;
}

/**
 * Field counts for the first records of a sample, under one candidate delimiter.
 *
 * A miniature of the parser in `parse.ts` — it has to be, because quoting is
 * exactly what the naive `split` gets wrong — but it counts rather than
 * collects, so a 64 KB sample costs nothing per candidate.
 *
 * The tail after the last line break is a record only when the sample is the
 * whole file. In a truncated sample it is a row cut in half, and scoring it
 * would punish the right answer for where the slice happened to land.
 */
function fieldCounts(sample: string, delimiter: string, truncated: boolean): number[] {
  const counts: number[] = [];
  const length = sample.length;
  let at = 0;
  let fields = 1;
  let inQuotes = false;
  let atRecordStart = true;

  while (at < length && counts.length < SNIFF_RECORDS) {
    const ch = sample[at];
    if (inQuotes) {
      if (ch === '"') {
        if (sample[at + 1] === '"') at += 1;
        else inQuotes = false;
      }
      at += 1;
      continue;
    }
    atRecordStart = false;
    if (ch === '"') {
      inQuotes = true;
      at += 1;
      continue;
    }
    if (ch === delimiter) {
      fields += 1;
      at += 1;
      continue;
    }
    if (ch === '\r' || ch === '\n') {
      if (ch === '\r' && sample[at + 1] === '\n') at += 1;
      at += 1;
      counts.push(fields);
      fields = 1;
      atRecordStart = true;
      continue;
    }
    at += 1;
  }

  if (!truncated && !atRecordStart && counts.length < SNIFF_RECORDS) counts.push(fields);
  return counts;
}

/**
 * Settle a dialect for one document, from what the user asked for, what the
 * file is called, and what is in it — in that order.
 */
export function resolveDialect(options: {
  /** The configured delimiter, or `auto`. */
  configured: string;
  /** The document's path, for the extension. */
  path: string;
  /** The document's text, for sniffing and for the line ending. */
  text: string;
}): Dialect {
  const { configured, path, text } = options;
  const delimiter =
    configured && configured !== 'auto'
      ? configured
      : (delimiterForPath(path) ?? sniffDelimiter(text));
  return { delimiter, quote: '"', newline: newlineOf(text) };
}
