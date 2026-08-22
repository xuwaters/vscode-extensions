/**
 * What a cell looks like it holds.
 *
 * Separated values have no types — every cell is a string, and the file cannot
 * tell you which strings were meant as numbers. So the grid guesses, for the
 * two places where a guess is worth more than it costs: which way a cell is
 * aligned, and how a column sorts. Nothing here ever changes what is written
 * back; `"007"` sorts as seven and is saved as `007`.
 */
export type CellKind = 'empty' | 'number' | 'date' | 'text';

/**
 * A number, if the cell reads as one.
 *
 * Deliberately wider than `Number(value)`: spreadsheets export thousands
 * separators, currency symbols, trailing percent signs and parenthesised
 * negatives, and a column of `$1,234.00` that sorts alphabetically is a column
 * that sorts wrong. Deliberately narrower too — `Number('')` is 0, `Number('
 * ')` is 0, and `Number('0x1f')` is 31, none of which a spreadsheet means.
 */
export function parseNumber(value: string): number | null {
  const text = value.trim();
  if (text === '') return null;

  const match = /^([-+(]?)\s*[$€£¥₹]?\s*([0-9][0-9,_ ]*(?:\.[0-9]+)?|\.[0-9]+)\s*(%?)\s*(\)?)$/.exec(
    text,
  );
  if (!match) {
    // Anything the friendly form missed but JavaScript still reads as a plain
    // decimal — `1e9`, `-2.5e-3`, `1.` — with hex, octal, binary, `Infinity`
    // and whitespace-only excluded by the shape test.
    if (!/^[-+]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][-+]?[0-9]+)?$/.test(text)) return null;
    const plain = Number(text);
    return Number.isFinite(plain) ? plain : null;
  }

  const [, sign, digits, percent, close] = match;
  // `(1,234)` is accounting for -1234, but only when the bracket is closed.
  if ((sign === '(') !== (close === ')')) return null;
  const magnitude = Number(digits!.replace(/[,_ ]/g, ''));
  if (!Number.isFinite(magnitude)) return null;
  const scaled = percent === '%' ? magnitude / 100 : magnitude;
  return sign === '-' || sign === '(' ? -scaled : scaled;
}

/**
 * A date, as milliseconds, if the cell reads as one.
 *
 * ISO 8601 and the two unambiguous slash forms only. `03/04/2024` is
 * deliberately *not* a date here: it is the fourth of March to half the world
 * and the third of April to the other half, and a sort that silently picks one
 * is worse than a sort that treats the column as text — where at least the
 * order is visibly alphabetical.
 */
export function parseDate(value: string): number | null {
  const text = value.trim();
  const match = /^(\d{4})[-/](\d{1,2})[-/](\d{1,2})(?:[T ](\d{1,2}):(\d{2})(?::(\d{2})(?:\.\d+)?)?)?/.exec(
    text,
  );
  if (!match) return null;
  const [, year, month, day, hour, minute, second] = match;
  const m = Number(month);
  const d = Number(day);
  if (m < 1 || m > 12 || d < 1 || d > 31) return null;
  const stamp = Date.UTC(
    Number(year),
    m - 1,
    d,
    Number(hour ?? 0),
    Number(minute ?? 0),
    Number(second ?? 0),
  );
  return Number.isFinite(stamp) ? stamp : null;
}

/** What a single cell reads as. */
export function kindOf(value: string): CellKind {
  if (value.trim() === '') return 'empty';
  if (parseNumber(value) !== null) return 'number';
  if (parseDate(value) !== null) return 'date';
  return 'text';
}

/**
 * The collator text falls back to.
 *
 * `numeric` is what makes `item2` come before `item10` — the ordering a person
 * reading a list of names with numbers in them expects, and the one a plain
 * code-unit comparison gets backwards. `sensitivity: 'base'` puts `Ábel` next
 * to `Abel` rather than after `Zoë`.
 */
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/**
 * Compare two cells for a sort, ascending.
 *
 * The rules are a spreadsheet's, because that is what the reader is expecting:
 *
 * * Blanks go last, and stay last when the sort is reversed — a column sorted
 *   descending should not open with its holes.
 * * Numbers compare as numbers, dates as dates.
 * * A number sorts before text, so a column of mostly-numbers with a stray
 *   `n/a` keeps its numeric order and parks the stray at the end of them.
 * * Text compares with a locale collator, digits inside it read as numbers.
 * * Ties fall back to the raw strings, so the order is total and a sort of
 *   `1.0` against `1.00` is at least stable and repeatable.
 *
 * The *blanks-last* rule is why this returns a comparison for ascending only,
 * and the sorter negates the rest: see `sortOrder`.
 */
export function compareValues(a: string, b: string): number {
  const aNum = parseNumber(a);
  const bNum = parseNumber(b);
  if (aNum !== null && bNum !== null) {
    return aNum === bNum ? tieBreak(a, b) : aNum - bNum;
  }
  if (aNum !== null) return -1;
  if (bNum !== null) return 1;

  const aDate = parseDate(a);
  const bDate = parseDate(b);
  if (aDate !== null && bDate !== null) {
    return aDate === bDate ? tieBreak(a, b) : aDate - bDate;
  }
  if (aDate !== null) return -1;
  if (bDate !== null) return 1;

  const text = collator.compare(a, b);
  return text !== 0 ? text : tieBreak(a, b);
}

function tieBreak(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Whether a cell counts as blank for the purpose of sorting. */
export function isBlank(value: string): boolean {
  return value.trim() === '';
}

export type SortDirection = 'asc' | 'desc';

/**
 * The order a column sorts its rows into, as a permutation of row indices.
 *
 * The single definition of "sorted" in this extension, and it has to be: the
 * page sorts its *view* with this, and the host writes that order into the file
 * with the same call rather than being sent a permutation to trust. A file whose
 * rows landed in an order the screen never showed would be the worst kind of bug
 * — the reader asked for what they were looking at.
 *
 * Two properties beyond the comparison itself:
 *
 * * `skip` leaves the first rows where they are. That is the header, which is a
 *   label rather than a row, and sorting it into the middle of the data is the
 *   classic spreadsheet accident.
 * * Blanks sink to the bottom *in both directions*, and the sort is stable —
 *   ties, blanks included, keep their file order. A column sorted descending
 *   should not open with its holes, and sorting twice should not shuffle the
 *   rows that compared equal.
 */
export function sortOrder(
  values: readonly string[],
  direction: SortDirection,
  skip = 0,
): number[] {
  const head = Math.min(Math.max(0, skip), values.length);
  const order = Array.from({ length: values.length }, (_, i) => i);
  const body = order.slice(head);
  const sign = direction === 'asc' ? 1 : -1;

  body.sort((a, b) => {
    const left = values[a] ?? '';
    const right = values[b] ?? '';
    const leftBlank = isBlank(left);
    const rightBlank = isBlank(right);
    if (leftBlank !== rightBlank) return leftBlank ? 1 : -1;
    if (leftBlank) return a - b;
    const compared = compareValues(left, right);
    return compared !== 0 ? sign * compared : a - b;
  });

  return [...order.slice(0, head), ...body];
}

/**
 * A column's spreadsheet letter: A, B, … Z, AA, AB.
 *
 * Bijective base 26 — there is no zero, so `Z` is followed by `AA` rather than
 * `BA`, which is what every spreadsheet does and what a naive base conversion
 * gets wrong. Shared by the grid's column heads and the text editor's status
 * bar, so the two name a column the same way.
 */
export function columnLabel(index: number): string {
  let label = '';
  let remaining = Math.max(0, Math.floor(index)) + 1;
  while (remaining > 0) {
    const digit = (remaining - 1) % 26;
    label = String.fromCharCode(65 + digit) + label;
    remaining = Math.floor((remaining - 1) / 26);
  }
  return label;
}

/**
 * Whether the first record names the columns.
 *
 * The question has no certain answer, so the test is the one a person applies
 * at a glance: a header row is full, is words rather than numbers, says
 * something different in each column, and is followed by rows that do not look
 * like it. Any one of those failing is enough to call it data — a false
 * negative costs a reader one keystroke, and a false positive hides their first
 * row of data inside the column titles.
 */
export function looksLikeHeader(rows: readonly (readonly string[])[]): boolean {
  const head = rows[0];
  if (!head || head.length === 0 || rows.length < 2) return false;

  const names = head.map((value) => value.trim());
  if (names.some((name) => name === '')) return false;
  if (names.some((name) => parseNumber(name) !== null || parseDate(name) !== null)) return false;
  if (new Set(names.map((name) => name.toLowerCase())).size !== names.length) return false;

  // A file whose second row is also all words is still probably headed — but a
  // file where *some* column below is plainly not text is certain of it, and
  // that is the signal worth waiting for. With no such column, fall back to
  // "the first row is words and the rest are not identical to it".
  const body = rows.slice(1, 21);
  for (let column = 0; column < names.length; column += 1) {
    const below = body.map((row) => row[column] ?? '').filter((value) => value.trim() !== '');
    if (below.length === 0) continue;
    if (below.every((value) => parseNumber(value) !== null || parseDate(value) !== null)) {
      return true;
    }
  }
  return body.length > 0 && !body.every((row) => names.every((name, i) => row[i]?.trim() === name));
}

/** What the grid's footer says about a selection. */
export interface Summary {
  /** Cells in the selection. */
  cells: number;
  /** Cells holding something. */
  filled: number;
  /** Cells reading as numbers. */
  numeric: number;
  sum: number;
  min: number;
  max: number;
}

/** Count, total and range of a run of cells — the spreadsheet status bar. */
export function summarize(values: Iterable<string>): Summary {
  const summary: Summary = {
    cells: 0,
    filled: 0,
    numeric: 0,
    sum: 0,
    min: Number.POSITIVE_INFINITY,
    max: Number.NEGATIVE_INFINITY,
  };
  for (const value of values) {
    summary.cells += 1;
    if (value !== '') summary.filled += 1;
    const number = parseNumber(value);
    if (number === null) continue;
    summary.numeric += 1;
    summary.sum += number;
    if (number < summary.min) summary.min = number;
    if (number > summary.max) summary.max = number;
  }
  return summary;
}
