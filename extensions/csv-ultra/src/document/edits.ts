import type { Dialect } from '../csv/dialect.js';
import { contentStart, type CsvField, type CsvRecord, type CsvTable } from '../csv/parse.js';
import {
  fieldsOf,
  writeRecord,
  writeTable,
  type QuoteStyle,
} from '../csv/serialize.js';
import { sortOrder, type SortDirection } from '../csv/values.js';
import type { CellPatch, GridEdit } from '../messages.js';

/**
 * One replacement, in character offsets into the document's text.
 *
 * Offsets rather than `vscode.Range` on purpose: this whole module is pure, and
 * turning offsets into positions is one line in the session that owns the
 * document. What is left here is arithmetic over a parsed file, which is exactly
 * the part worth testing — every function below decides which bytes of somebody's
 * data get overwritten.
 */
export interface OffsetEdit {
  start: number;
  end: number;
  newText: string;
}

/**
 * Turn what the page asked for into edits to the document.
 *
 * Two shapes of answer come out of here, and the difference is deliberate:
 *
 * * **A cell or a row** rewrites the one record it touches. Every other byte of
 *   the file — its other rows' quoting, its line endings, its trailing newline,
 *   the raggedness of a file whose rows are not all the same length — is left
 *   exactly where it was. Editing one cell of a 200 MB file is one splice.
 *
 * * **A column, a sort, a delimiter** rewrites the file. There is no way to
 *   insert a column without touching every record, so the choice is a hundred
 *   thousand splices or one; one is simpler, atomic, a single undo step, and has
 *   no way to leave two offsets disagreeing part way through. The cost is that a
 *   file with mixed line endings comes out consistent, and quoting is normalised
 *   to `style` — which is what `preserve` is for.
 *
 * Returns an empty list for an edit that would change nothing, so the caller can
 * skip the document write and the undo entry that comes with it.
 */
export function editsFor(
  table: CsvTable,
  length: number,
  edit: GridEdit,
  style: QuoteStyle,
): OffsetEdit[] {
  switch (edit.kind) {
    case 'cells':
      return setCells(table, length, edit.patches, style);
    case 'insertRows':
      return insertRows(table, length, edit.at, edit.rows, style);
    case 'deleteRows':
      return deleteRows(table, length, edit.rows);
    case 'insertColumns':
      return insertColumns(table, length, edit.at, edit.count, style);
    case 'deleteColumns':
      return deleteColumns(table, length, edit.columns, style);
    case 'sort':
      return applySort(table, length, edit.column, edit.direction, edit.header, style);
  }
}

/**
 * Write cells.
 *
 * Patches are grouped by record, so a pasted block of a thousand cells over
 * fifty rows is fifty splices rather than a thousand. A record only long enough
 * to reach column 3 is padded out to a written column 9 — the empty fields in
 * between have to be spelled out, because a file cannot say "column 9 is X" any
 * other way.
 *
 * Rows *past the end of the file* are not an error: pasting a block near the
 * bottom of a table is meant to extend it, and refusing would silently drop
 * whatever fell off. They become records appended in one piece.
 */
export function setCells(
  table: CsvTable,
  length: number,
  patches: readonly CellPatch[],
  style: QuoteStyle,
): OffsetEdit[] {
  const { records } = table;
  const byRow = new Map<number, CellPatch[]>();
  let lastRow = records.length - 1;
  for (const patch of patches) {
    const group = byRow.get(patch.row);
    if (group) group.push(patch);
    else byRow.set(patch.row, [patch]);
    if (patch.row > lastRow) lastRow = patch.row;
  }

  const edits: OffsetEdit[] = [];

  for (const [row, group] of [...byRow].sort((a, b) => a[0] - b[0])) {
    if (row >= records.length) continue;
    const record = records[row]!;
    const fields = applyToFields(record.fields, group);
    if (!fields) continue;
    edits.push({
      start: record.start,
      end: record.end,
      newText: writeRecord(fields, table.dialect, style),
    });
  }

  // Anything below the last record, as new rows in one insertion.
  if (lastRow >= records.length) {
    const appended: string[][] = [];
    for (let row = records.length; row <= lastRow; row += 1) {
      const width = Math.max(
        1,
        ...(byRow.get(row) ?? []).map((patch) => patch.column + 1),
      );
      const values = new Array<string>(width).fill('');
      for (const patch of byRow.get(row) ?? []) values[patch.column] = patch.value;
      appended.push(values);
    }
    edits.push(...insertRows(table, length, records.length, appended, style));
  }

  return edits;
}

/**
 * Apply patches to one record's fields, or return null if nothing changed.
 *
 * The null is what stops a click-in-click-out of a cell, or a paste of the same
 * values over themselves, from marking the file dirty and pushing an undo entry
 * that undoes nothing.
 */
function applyToFields(
  fields: readonly CsvField[],
  patches: readonly CellPatch[],
): CsvField[] | null {
  let width = fields.length;
  for (const patch of patches) {
    // A blank written past the end of a short record is already true of it.
    if (patch.column + 1 > width && patch.value !== '') width = patch.column + 1;
  }

  const next: CsvField[] = new Array(width);
  for (let i = 0; i < width; i += 1) next[i] = fields[i] ?? { value: '', quoted: false };

  let changed = width !== fields.length;
  for (const patch of patches) {
    const target = next[patch.column];
    if (!target || target.value === patch.value) continue;
    next[patch.column] = { value: patch.value, quoted: target.quoted };
    changed = true;
  }
  return changed ? next : null;
}

/**
 * Insert whole records.
 *
 * `at` beyond the last record appends, which is the interesting case because
 * where the text ends depends on how the file ends. A file with a trailing
 * newline gets `row + newline`; a file without one gets `newline + row`, so
 * appending never joins the new row onto the end of the old last one — and a
 * file that ended without a terminator still does.
 */
export function insertRows(
  table: CsvTable,
  length: number,
  at: number,
  rows: readonly string[][],
  style: QuoteStyle,
): OffsetEdit[] {
  if (rows.length === 0) return [];
  const { dialect, records } = table;
  const body = rows.map((values) => writeRecord(fieldsOf(values), dialect, style));

  if (at >= records.length) {
    const opensWithBreak = records.length > 0 && !table.trailingNewline;
    return [
      {
        start: length,
        end: length,
        newText:
          (opensWithBreak ? dialect.newline : '') +
          body.join(dialect.newline) +
          (table.trailingNewline || records.length === 0 ? dialect.newline : ''),
      },
    ];
  }

  const target = records[Math.max(0, at)]!;
  return [
    {
      start: target.start,
      end: target.start,
      newText: body.join(dialect.newline) + dialect.newline,
    },
  ];
}

/**
 * Delete whole records, terminator and all.
 *
 * A record's extent runs to the start of the next one, so deleting rows 3 and 4
 * of five leaves no blank line behind. The *last* record has no next one, so it
 * takes everything to the end of the file — which is its own terminator, if it
 * had one.
 *
 * Adjacent deletions are merged into one edit. Not for tidiness: a range that
 * ends exactly where the next begins is legal in a `WorkspaceEdit` but a run of
 * a thousand of them is a thousand entries VSCode has to sort and apply, and the
 * merged form is the same result in one.
 */
export function deleteRows(
  table: CsvTable,
  length: number,
  rows: readonly number[],
): OffsetEdit[] {
  const { records } = table;
  const wanted = [...new Set(rows)]
    .filter((row) => row >= 0 && row < records.length)
    .sort((a, b) => a - b);
  if (wanted.length === 0) return [];

  const edits: OffsetEdit[] = [];
  for (const row of wanted) {
    const record = records[row]!;
    const next = records[row + 1];
    const start = record.start;
    const end = next ? next.start : length;
    const last = edits[edits.length - 1];
    if (last && last.end === start) last.end = end;
    else edits.push({ start, end, newText: '' });
  }

  // Deleting the tail of a file that had no trailing newline leaves the
  // terminator of the row above dangling. Reach back over it, so a file that
  // ended without one still does.
  const tail = edits[edits.length - 1];
  if (tail && tail.end === length && !table.trailingNewline && tail.start > 0) {
    const previous = records[wanted[0]! - 1];
    if (previous) tail.start = previous.end;
  }

  return edits;
}

/** Insert empty columns, before column `at`. */
export function insertColumns(
  table: CsvTable,
  length: number,
  at: number,
  count: number,
  style: QuoteStyle,
): OffsetEdit[] {
  if (count < 1 || table.records.length === 0) return [];
  const blanks = fieldsOf(new Array<string>(count).fill(''));
  return [
    rewrite(
      table,
      length,
      table.records.map((record) => {
        // A record that stops short of `at` already has nothing there, and
        // padding it out would materialise empty fields the file never had. A
        // record that reaches exactly `at` is a different matter: inserting a
        // column to the right of the last one has to be written down somewhere,
        // or the new column does not exist and there is nothing to type into.
        if (record.fields.length < at) return record;
        const fields = [...record.fields];
        fields.splice(at, 0, ...blanks);
        return { ...record, fields };
      }),
      style,
    ),
  ];
}

/** Remove columns. */
export function deleteColumns(
  table: CsvTable,
  length: number,
  columns: readonly number[],
  style: QuoteStyle,
): OffsetEdit[] {
  const doomed = new Set(columns.filter((column) => column >= 0));
  if (doomed.size === 0 || table.records.length === 0) return [];
  return [
    rewrite(
      table,
      length,
      table.records.map((record) => {
        const fields = record.fields.filter((_, index) => !doomed.has(index));
        // A record is never *no* fields: an empty row is one empty field, which
        // is how a blank line reads back.
        return { ...record, fields: fields.length > 0 ? fields : fieldsOf(['']) };
      }),
      style,
    ),
  ];
}

/**
 * Write the view's sort order into the file.
 *
 * The order is recomputed here from the same `sortOrder` the page sorted its
 * view with, rather than the page sending its permutation over. Both halves then
 * cannot disagree by construction, and no message can ask for an arbitrary
 * shuffle of somebody's rows.
 */
export function applySort(
  table: CsvTable,
  length: number,
  column: number,
  direction: SortDirection,
  header: boolean,
  style: QuoteStyle,
): OffsetEdit[] {
  const { records } = table;
  if (records.length < 2) return [];
  const keys = records.map((record) => record.fields[column]?.value ?? '');
  const order = sortOrder(keys, direction, header ? 1 : 0);
  if (order.every((from, to) => from === to)) return [];
  return [rewrite(table, length, order.map((from) => records[from]!), style)];
}

/**
 * Rewrite the file with a different delimiter.
 *
 * The values are untouched; what changes is which of them need quotes. A field
 * holding a comma needed them as CSV and does not as TSV, and one holding a tab
 * is the other way round — so this is not a search and replace, and doing it as
 * one is how a converted file ends up with a row that is one field too long.
 */
export function convertDialect(
  table: CsvTable,
  length: number,
  to: Dialect,
  style: QuoteStyle,
): OffsetEdit[] {
  if (table.records.length === 0) return [];
  return [
    {
      start: contentStart(table),
      end: length,
      newText: writeTable(table.records, to, style, table.trailingNewline),
    },
  ];
}

/** The whole file, from past the byte-order mark to the end. */
function rewrite(
  table: CsvTable,
  length: number,
  records: readonly CsvRecord[],
  style: QuoteStyle,
): OffsetEdit {
  return {
    start: contentStart(table),
    end: length,
    newText: writeTable(records, table.dialect, style, table.trailingNewline),
  };
}

/**
 * Apply edits to a string — the pure twin of what VSCode does to the document.
 *
 * Only the tests use it, and that is the point: an edit list is worth nothing
 * unless the file it produces is the file the reader expected, and asserting on
 * offsets proves nothing about that. Applied right to left so earlier offsets
 * stay valid.
 */
export function applyEdits(text: string, edits: readonly OffsetEdit[]): string {
  let out = text;
  for (const edit of [...edits].sort((a, b) => b.start - a.start)) {
    out = out.slice(0, edit.start) + edit.newText + out.slice(edit.end);
  }
  return out;
}
