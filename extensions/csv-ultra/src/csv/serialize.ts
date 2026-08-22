import type { Dialect } from './dialect.js';
import type { CsvField, CsvRecord } from './parse.js';

/**
 * When a field is written with quotes around it.
 *
 * `preserve` is the default because it is the only one that leaves a file
 * looking like itself. Editing one cell of a file that quotes everything should
 * not produce a row that quotes nothing, and vice versa — the diff of a
 * one-cell edit should be one cell.
 */
export type QuoteStyle = 'preserve' | 'minimal' | 'always';

/**
 * Whether a value *has* to be quoted, whatever the style says.
 *
 * The delimiter, the quote character and line breaks would all be read back as
 * structure. Leading and trailing spaces are on the list for a softer reason:
 * RFC 4180 says they are part of the value, plenty of readers disagree, and
 * quoting them costs two characters and settles the argument.
 */
export function needsQuoting(value: string, dialect: Dialect): boolean {
  return (
    value.includes(dialect.delimiter) ||
    value.includes(dialect.quote) ||
    value.includes('\n') ||
    value.includes('\r') ||
    /^[ \t]|[ \t]$/.test(value)
  );
}

/** One field, as it goes back into the file. */
export function writeField(field: CsvField, dialect: Dialect, style: QuoteStyle): string {
  const { value } = field;
  const quote =
    needsQuoting(value, dialect) ||
    style === 'always' ||
    (style === 'preserve' && field.quoted);
  if (!quote) return value;
  return dialect.quote + value.split(dialect.quote).join(dialect.quote + dialect.quote) + dialect.quote;
}

/** One record, without its line terminator. */
export function writeRecord(
  fields: readonly CsvField[],
  dialect: Dialect,
  style: QuoteStyle,
): string {
  let out = '';
  for (let i = 0; i < fields.length; i += 1) {
    if (i > 0) out += dialect.delimiter;
    out += writeField(fields[i]!, dialect, style);
  }
  return out;
}

/** A record built from plain values — a pasted row, or a freshly inserted one. */
export function fieldsOf(values: readonly string[], quoted = false): CsvField[] {
  return values.map((value) => ({ value, quoted }));
}

/** One record from plain values, without its line terminator. */
export function writeValues(
  values: readonly string[],
  dialect: Dialect,
  style: QuoteStyle,
): string {
  return writeRecord(fieldsOf(values), dialect, style);
}

/**
 * A whole file.
 *
 * Used for the operations that touch every record anyway — a column inserted or
 * removed, a sort written down, a delimiter converted — where rebuilding the
 * text is both simpler and *safer* than a hundred thousand splices: one edit,
 * one undo step, no chance of two offsets disagreeing.
 *
 * Cell and row edits do not come through here. They rewrite the one record they
 * touch and leave every other byte of the file exactly where it was.
 */
export function writeTable(
  records: readonly CsvRecord[],
  dialect: Dialect,
  style: QuoteStyle,
  trailingNewline: boolean,
): string {
  let out = '';
  for (let i = 0; i < records.length; i += 1) {
    if (i > 0) out += dialect.newline;
    out += writeRecord(records[i]!.fields, dialect, style);
  }
  if (trailingNewline && records.length > 0) out += dialect.newline;
  return out;
}
