import type * as vscode from 'vscode';
import type { Dialect } from '../csv/dialect.js';
import { parse, type CsvTable } from '../csv/parse.js';

/** A document, parsed, plus the one fact about its text that the edits need. */
export interface DocumentTable {
  readonly table: CsvTable;
  /** The document's length in characters, which is where an append goes. */
  readonly length: number;
}

/**
 * The parse of each open document, kept until the document changes.
 *
 * Two things want it and neither can afford to re-parse: the session, which
 * turns a cell edit into an offset into the file, and the rainbow decorator,
 * which runs on every scroll of every visible text editor. Keyed by the
 * document's version, so a cache hit is exact rather than probable — there is no
 * invalidation to get wrong, because a changed document *is* a different key.
 *
 * What is deliberately *not* kept is the text. The parse holds every value in
 * the file already, and the only other thing the edits need is where the end is
 * — so a cached table costs its own size and not a second copy of the document.
 * The price is one `getText()` and one parse per edit, which is a few tens of
 * milliseconds on a file large enough to notice, once per committed cell.
 */
export class TableCache {
  private readonly entries = new Map<string, Entry>();

  /**
   * @param limit How many documents to remember. A handful: the point is the
   * tab in front of the reader and the two behind it, not a history.
   */
  constructor(private readonly limit = 6) {}

  /** The parse of a document under a dialect, from cache when it can be. */
  of(document: vscode.TextDocument, dialect: Dialect): DocumentTable {
    const key = document.uri.toString();
    const cached = this.entries.get(key);
    if (
      cached &&
      cached.version === document.version &&
      cached.delimiter === dialect.delimiter &&
      cached.quote === dialect.quote
    ) {
      // Freshen the insertion order — the eviction below is oldest-first.
      this.entries.delete(key);
      this.entries.set(key, cached);
      return cached;
    }

    const text = document.getText();
    const entry: Entry = {
      version: document.version,
      delimiter: dialect.delimiter,
      quote: dialect.quote,
      table: parse(text, dialect),
      length: text.length,
    };
    this.entries.delete(key);
    this.entries.set(key, entry);
    while (this.entries.size > this.limit) {
      const oldest = this.entries.keys().next();
      if (oldest.done) break;
      this.entries.delete(oldest.value);
    }
    return entry;
  }

  /** Drop a document — it closed, or it is about to be read a different way. */
  forget(uri: vscode.Uri): void {
    this.entries.delete(uri.toString());
  }

  dispose(): void {
    this.entries.clear();
  }
}

interface Entry extends DocumentTable {
  version: number;
  delimiter: string;
  quote: string;
}

/**
 * Which record an offset falls in, or -1 for an offset before the first one.
 *
 * Binary search over the record starts, which is what makes colouring the text
 * editor cheap: the visible range is two offsets, and the records between them
 * are a slice rather than a scan from the top of the file. An offset inside a
 * record's line terminator belongs to that record, which is the answer a caller
 * asking "what am I looking at" wants.
 */
export function recordIndexAt(table: CsvTable, offset: number): number {
  const { records } = table;
  let low = 0;
  let high = records.length - 1;
  let found = -1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if (records[middle]!.start <= offset) {
      found = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return found;
}

/**
 * Which column an offset falls in, within a record.
 *
 * Counts delimiters outside quotes, which is the same walk `fieldSpans` does but
 * without building the spans — this answers "what column is the cursor in" for
 * the status bar, thousands of times a session, and the record it is asked about
 * is usually one line long.
 */
export function columnAt(record: string, offset: number, dialect: Dialect): number {
  const { delimiter, quote } = dialect;
  const stop = Math.min(offset, record.length);
  let column = 0;
  let at = 0;
  let quoted = false;
  while (at < stop) {
    const character = record[at];
    if (quoted) {
      if (character === quote) {
        if (record[at + 1] === quote) at += 1;
        else quoted = false;
      }
    } else if (character === quote) {
      quoted = true;
    } else if (character === delimiter) {
      column += 1;
    }
    at += 1;
  }
  return column;
}
