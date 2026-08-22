import { fieldSpans, type CsvTable } from '../csv/parse.js';

/** How many colours the palette cycles through. Matches `contributes.colors`. */
export const RAINBOW_COLORS = 10;

/** One colour's share of the document, as offsets. */
export interface ColorSpan {
  start: number;
  end: number;
}

/**
 * Which colour each field in a window of records gets, as document offsets.
 *
 * Pure, and the whole of the decision: a field's colour is its column index
 * modulo the palette, and its extent is the field *as written* — quotes
 * included, so the paint covers what the reader sees rather than the value
 * inside it.
 *
 * Zero-width fields are dropped. An empty field between two delimiters has
 * nothing to colour, and a decoration over an empty range is a marker VSCode
 * still has to lay out.
 *
 * @param recordText Hands back one record's own text. A callback rather than the
 * document, because the window is a slice and the rest of the file — which can
 * be a hundred megabytes — has no business being read to paint a screen.
 */
export function paintPlan(
  table: CsvTable,
  from: number,
  to: number,
  recordText: (index: number) => string,
  colors = RAINBOW_COLORS,
): ColorSpan[][] {
  const buckets: ColorSpan[][] = Array.from({ length: colors }, () => []);
  const first = Math.max(0, from);
  const last = Math.min(table.records.length - 1, to);

  for (let index = first; index <= last; index += 1) {
    const record = table.records[index]!;
    const spans = fieldSpans(recordText(index), table.dialect);
    for (let column = 0; column < spans.length; column += 1) {
      const span = spans[column]!;
      if (span.end <= span.start) continue;
      buckets[column % colors]!.push({
        start: record.start + span.start,
        end: record.start + span.end,
      });
    }
  }

  return buckets;
}
