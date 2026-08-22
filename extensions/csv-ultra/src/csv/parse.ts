import type { Dialect } from './dialect.js';

/** One field of one record, as it was found in the file. */
export interface CsvField {
  /** The value a cell shows: quotes stripped, doubled quotes collapsed to one. */
  readonly value: string;
  /**
   * Whether the file spelled this field with quotes.
   *
   * Kept so a rewrite can put them back — see `preserve` in `serialize.ts`. A
   * spreadsheet that quotes every field and a script that quotes none are both
   * common, and an edited row that changes habit stands out in a diff for a
   * reason that has nothing to do with what was edited.
   */
  readonly quoted: boolean;
}

/**
 * One record: the fields, and where in the file they came from.
 *
 * A record is not a line. A quoted field may contain line breaks, so a record
 * can span several of them — which is exactly why `start` and `end` are
 * character offsets into the source rather than a line number.
 *
 * `end` is one past the last character of the record's *content*: it stops
 * before the line terminator, so `text.slice(start, end)` is the record and
 * nothing else, and replacing that span rewrites one row without touching how
 * the file ends its lines.
 */
export interface CsvRecord {
  readonly fields: CsvField[];
  readonly start: number;
  readonly end: number;
}

/** A whole file, parsed. */
export interface CsvTable {
  readonly records: CsvRecord[];
  /** Fields in the widest record — the grid's column count. */
  readonly columns: number;
  readonly dialect: Dialect;
  /** Whether the file ends with a line terminator, as almost all of them do. */
  readonly trailingNewline: boolean;
  /**
   * Whether the file opens with a byte-order mark.
   *
   * Excel writes one and reads one; it is not part of the first field's value,
   * and it is not part of the first record. Every offset in `records` is past
   * it, so a rewrite that starts at {@link contentStart} leaves it alone.
   */
  readonly bom: boolean;
}

/** The offset the file's data starts at: past a byte-order mark, if there is one. */
export function contentStart(table: CsvTable): number {
  return table.bom ? 1 : 0;
}

/** The decoded values of one record, padded to `columns` if asked. */
export function valuesOf(record: CsvRecord, columns = 0): string[] {
  const values = record.fields.map((field) => field.value);
  while (values.length < columns) values.push('');
  return values;
}

/** Every record's values, as the rectangular grid the page draws. */
export function gridOf(table: CsvTable): string[][] {
  return table.records.map((record) => valuesOf(record, table.columns));
}

/**
 * Parse separated values.
 *
 * RFC 4180 where the file agrees with it, and forgiving everywhere it does not,
 * because a parser that refuses a file is a parser that cannot show it:
 *
 * * A field that opens with a quote runs to the matching close quote, doubled
 *   quotes inside standing for one, line breaks and delimiters included.
 * * A quote that never closes swallows the rest of the file rather than
 *   throwing — which is what a half-typed row in a file being edited looks
 *   like, and the next keystroke fixes it.
 * * Text *after* a closing quote (`"a"b`) is appended to the value instead of
 *   being dropped. Nothing writes that on purpose; something has written it.
 * * `\n`, `\r\n` and a lone `\r` all end a record. The file's own habit is what
 *   gets written back — see `Dialect.newline` — but reading is catholic.
 * * A blank line is a record of one empty field, the way a spreadsheet shows
 *   it. Only a *trailing* line break ends the file rather than starting a row.
 *
 * The unquoted path — the overwhelming majority of fields — is a regex scan to
 * the next delimiter or line break rather than a character loop, which is what
 * keeps a megabyte of CSV inside a frame.
 */
export function parse(text: string, dialect: Dialect): CsvTable {
  const { delimiter, quote } = dialect;
  const length = text.length;
  const bom = text.charCodeAt(0) === 0xfeff;

  const records: CsvRecord[] = [];
  let columns = 0;
  let at = bom ? 1 : 0;

  // One scanner, reused across every field: `lastIndex` is moved by hand, so
  // there is no per-field allocation and no per-character step.
  const stopper = new RegExp(`[${escapeForClass(delimiter)}\\r\\n]`, 'g');
  const nextStop = (from: number): number => {
    stopper.lastIndex = from;
    const found = stopper.exec(text);
    return found ? found.index : length;
  };

  while (at < length) {
    const start = at;
    const fields: CsvField[] = [];
    let end = at;

    for (;;) {
      let value: string;
      let quoted = false;

      if (text[at] === quote) {
        quoted = true;
        at += 1;
        // Built by pieces rather than by character: the common case is one
        // slice, and a field with doubled quotes is one slice per pair.
        let parts = '';
        for (;;) {
          const close = text.indexOf(quote, at);
          if (close < 0) {
            parts += text.slice(at);
            at = length;
            break;
          }
          if (text[close + 1] === quote) {
            parts += text.slice(at, close + 1);
            at = close + 2;
            continue;
          }
          parts += text.slice(at, close);
          at = close + 1;
          break;
        }
        // Anything between the closing quote and the next delimiter belongs to
        // nobody; keep it rather than lose it.
        const stop = nextStop(at);
        if (stop > at) parts += text.slice(at, stop);
        at = stop;
        value = parts;
      } else {
        const stop = nextStop(at);
        value = text.slice(at, stop);
        at = stop;
      }

      fields.push({ value, quoted });
      end = at;

      if (at < length && text[at] === delimiter) {
        at += 1;
        continue;
      }
      break;
    }

    records.push({ fields, start, end });
    if (fields.length > columns) columns = fields.length;

    if (at < length) {
      if (text[at] === '\r') {
        at += 1;
        if (text[at] === '\n') at += 1;
      } else {
        at += 1;
      }
    }
  }

  const last = text.charCodeAt(length - 1);
  return {
    records,
    columns,
    dialect,
    trailingNewline: records.length > 0 && (last === 10 || last === 13),
    bom,
  };
}

/**
 * Where each field of one record sits inside it, as offsets relative to the
 * record's start.
 *
 * The parser deliberately keeps no per-field offsets: two numbers per *cell* is
 * a lot of memory for a table nobody is looking at most of. But colouring a
 * column in the text editor needs exactly that, for the handful of records on
 * screen — so it is computed on demand, for one record at a time, from the
 * record's own text.
 *
 * The span covers the field as written, quotes included, so a decoration drawn
 * over it colours what the reader sees.
 */
export function fieldSpans(
  record: string,
  dialect: Dialect,
): Array<{ start: number; end: number }> {
  const { delimiter, quote } = dialect;
  const spans: Array<{ start: number; end: number }> = [];
  const length = record.length;
  let at = 0;

  for (;;) {
    const start = at;
    if (record[at] === quote) {
      at += 1;
      for (;;) {
        const close = record.indexOf(quote, at);
        if (close < 0) {
          at = length;
          break;
        }
        if (record[close + 1] === quote) {
          at = close + 2;
          continue;
        }
        at = close + 1;
        break;
      }
    }
    while (at < length && record[at] !== delimiter) at += 1;

    spans.push({ start, end: at });
    if (at >= length) return spans;
    at += 1;
    // A record ending in the delimiter has one more (empty) field after it.
    if (at >= length) {
      spans.push({ start: at, end: at });
      return spans;
    }
  }
}

/** Make one character safe to drop inside a `[…]` character class. */
function escapeForClass(character: string): string {
  return character.replace(/[\\\]^-]/g, '\\$&');
}
