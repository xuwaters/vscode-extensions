import type { Dialect } from './csv/dialect.js';
import type { SortDirection } from './csv/values.js';

export type { SortDirection };

/**
 * The host ⇄ webview protocol.
 *
 * Imported by both sides, so a change to one is a type error in the other.
 *
 * Every `WebviewToHost` message is shape-validated host-side by a hand-written
 * guard per variant — no `any` dispatch. A webview is a hostile input boundary
 * even when we wrote the code on the other side of it, and this one is a boundary
 * with a *write* on the far side: what comes back through here is turned into
 * edits to the reader's file. A row index off by a thousand is not a rendering
 * bug, it is data loss, so the guards below check ranges as well as types.
 */

/** The parts of the configuration the page needs. */
export interface GridSettings {
  /** `auto` lets `looksLikeHeader` decide; the others are the reader's word. */
  headerRow: 'auto' | 'always' | 'never';
  rowHeight: number;
  columnWidth: number;
  maxColumnWidth: number;
  autoFitOnOpen: boolean;
  /** 0 follows the editor's own font size. */
  fontSize: number;
  fontFamily: 'editor' | 'ui';
  wrap: boolean;
  zebraStripes: boolean;
  alignNumbers: boolean;
}

/**
 * How a reader left a table, so reopening it is not starting over.
 *
 * Widths and heights are pairs rather than an object keyed by number: JSON has
 * no integer keys, and a sparse array of a million rows is not a message.
 */
export interface GridLayout {
  /** `[column, width]`, only for columns that have been sized. */
  widths: Array<[number, number]>;
  /** `[row, height]`, only for rows that have been sized. */
  heights: Array<[number, number]>;
  /** The view's sort, which is not a change to the file. */
  sort: { column: number; direction: SortDirection } | null;
  /** The reader's word about the header row, overriding the guess. */
  header: boolean | null;
  wrap: boolean | null;
  /** A font size set with the keyboard, overriding the setting. */
  fontSize: number | null;
  scrollTop: number;
  scrollLeft: number;
  activeRow: number;
  activeColumn: number;
}

/**
 * What the page may ask the *host* to do.
 *
 * A closed union rather than a command string, and validated against it, because
 * the other side of this message is `executeCommand`: a page that could name any
 * command could run any command in the window. These five are the ones with a
 * control in the table's own chrome — the dialogs and the editor swap, which a
 * webview cannot do for itself.
 */
export type HostCommand =
  | 'openInTextEditor'
  | 'setDelimiter'
  | 'convertDelimiter'
  | 'saveAsTsv'
  | 'saveAsCsv';

export const HOST_COMMANDS: readonly HostCommand[] = [
  'openInTextEditor',
  'setDelimiter',
  'convertDelimiter',
  'saveAsTsv',
  'saveAsCsv',
];

/** What the title bar, a key, or the palette can ask the table to do. */
export type GridCommand =
  | 'find'
  | 'findNext'
  | 'findPrevious'
  | 'goToRow'
  | 'toggleHeaderRow'
  | 'sortAscending'
  | 'sortDescending'
  | 'clearSort'
  | 'applySort'
  | 'insertRowAbove'
  | 'insertRowBelow'
  | 'deleteRows'
  | 'insertColumnLeft'
  | 'insertColumnRight'
  | 'deleteColumns'
  | 'toggleWrap'
  | 'autoFitColumns'
  | 'resetLayout'
  | 'rowHeightIncrease'
  | 'rowHeightDecrease'
  | 'fontSizeIncrease'
  | 'fontSizeDecrease'
  | 'fontSizeReset';

export const GRID_COMMANDS: readonly GridCommand[] = [
  'find',
  'findNext',
  'findPrevious',
  'goToRow',
  'toggleHeaderRow',
  'sortAscending',
  'sortDescending',
  'clearSort',
  'applySort',
  'insertRowAbove',
  'insertRowBelow',
  'deleteRows',
  'insertColumnLeft',
  'insertColumnRight',
  'deleteColumns',
  'toggleWrap',
  'autoFitColumns',
  'resetLayout',
  'rowHeightIncrease',
  'rowHeightDecrease',
  'fontSizeIncrease',
  'fontSizeDecrease',
  'fontSizeReset',
];

/** One cell, written. */
export interface CellPatch {
  /** Index into the file's records — never a view row. */
  row: number;
  column: number;
  value: string;
}

/**
 * A change to the document, as the page asks for it.
 *
 * Deliberately *semantic* rather than textual: the page says "these cells now
 * hold this", and the host decides which bytes that is. The page has the
 * selection and the reader's intent; the host has the file's offsets, its
 * quoting habits and its line endings. Neither half can do the other's job, and
 * a protocol of text splices would have put the file's structure in the webview,
 * where a bug becomes a corrupted file rather than a wrong-looking table.
 *
 * Every row index here is an index into the file's records, in file order. The
 * page maps its own view — which may be sorted — before it asks.
 */
export type GridEdit =
  | { kind: 'cells'; patches: CellPatch[] }
  /** `at` may be one past the last record, which appends. */
  | { kind: 'insertRows'; at: number; rows: string[][] }
  | { kind: 'deleteRows'; rows: number[] }
  | { kind: 'insertColumns'; at: number; count: number }
  | { kind: 'deleteColumns'; columns: number[] }
  /**
   * Write the view's sort into the file. The comparison is *not* sent: the host
   * re-derives the order with the same pure code the page sorted with, so there
   * is no way for a permutation to arrive that does not describe a sort.
   */
  | { kind: 'sort'; column: number; direction: SortDirection; header: boolean };

/** Why the page is being handed text. */
export type LoadReason =
  /** First load of a tab. */
  | 'open'
  /** The document changed under us — a text editor, a revert, another tab. */
  | 'external'
  /** The reader chose a different delimiter for this view. */
  | 'delimiter';

/** Host → webview. */
export type HostToWebview =
  | {
      type: 'load';
      name: string;
      text: string;
      dialect: Dialect;
      settings: GridSettings;
      reason: LoadReason;
      /** Absent unless this window has shown the file before. */
      layout?: GridLayout;
    }
  /**
   * Land on a cell, overriding whatever the layout remembered.
   *
   * Sent when the reader came from a text editor on the same file: the cursor
   * was in a cell, and arriving in the table two thousand rows away from it is
   * not opening the same file, it is losing your place. Row and column are in
   * *file* order; the page maps them through its own sort.
   */
  | { type: 'select'; row: number; column: number }
  | { type: 'settings'; settings: GridSettings }
  | { type: 'command'; command: GridCommand }
  /** The answer to `requestPaste` — the host owns the clipboard, not the page. */
  | { type: 'paste'; text: string }
  /** The file is larger than the reader allowed a table to be built from. */
  | { type: 'refused'; bytes: number; limit: number }
  /**
   * The tab came back to the front. A hidden webview is given no animation
   * frames, and the grid draws on them, so a tab returning may be carrying a
   * body that stopped painting half way.
   */
  | { type: 'visible' }
  /**
   * The tab became the active one. VSCode focuses the page but nothing inside
   * it, and the keys that move the selection act on whatever holds the focus.
   */
  | { type: 'focus' }
  | { type: 'hostError'; message: string };

/** Webview → host. */
export type WebviewToHost =
  | { type: 'ready' }
  | { type: 'edit'; edit: GridEdit }
  /** Throttled; drives the status bar and what a reopen restores. */
  | { type: 'place'; place: GridPlace }
  /** A cell editor opened or closed; gates the keybindings that would fight it. */
  | { type: 'editing'; editing: boolean }
  | { type: 'clipboard'; text: string }
  | { type: 'requestPaste' }
  /** A control in the table's chrome that only the host can carry out. */
  | { type: 'run'; command: HostCommand }
  | { type: 'error'; message: string; context: string };

/** Where the reader is, and what the table looks like from there. */
export interface GridPlace {
  /** 1-based, as the row headers count. */
  row: number;
  /** 0-based. */
  column: number;
  /** What the column is called — its header, or its letter. */
  columnName: string;
  rows: number;
  columns: number;
  /** Cells in the selection, 1 for a single cell. */
  selected: number;
  layout: GridLayout;
}

/** Largest row or column index the protocol will carry. */
export const MAX_INDEX = 100_000_000;

/** Longest cell value the protocol will carry, in characters. */
export const MAX_CELL = 1_000_000;

/** Most cells one edit may carry — a paste, in practice. */
export const MAX_PATCHES = 2_000_000;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isIndex(value: unknown): value is number {
  return (
    typeof value === 'number' &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= MAX_INDEX
  );
}

function isSize(value: unknown, max: number): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= max;
}

function isCellValue(value: unknown): value is string {
  return typeof value === 'string' && value.length <= MAX_CELL;
}

function isDirection(value: unknown): value is SortDirection {
  return value === 'asc' || value === 'desc';
}

function parsePairs(value: unknown, max: number): Array<[number, number]> | null {
  if (!Array.isArray(value) || value.length > 100_000) return null;
  const pairs: Array<[number, number]> = [];
  for (const entry of value) {
    if (!Array.isArray(entry) || entry.length !== 2) return null;
    if (!isIndex(entry[0]) || !isSize(entry[1], max)) return null;
    pairs.push([entry[0], entry[1]]);
  }
  return pairs;
}

/**
 * Validate a layout.
 *
 * Read leniently where a missing answer is still a usable one — a layout parked
 * by an older version predates whatever it is missing — and strictly where a
 * wrong answer would place the reader somewhere that does not exist.
 */
export function parseLayout(value: unknown): GridLayout | null {
  if (!isObject(value)) return null;
  const widths = parsePairs(value.widths, 4000);
  const heights = parsePairs(value.heights, 4000);
  if (!widths || !heights) return null;
  if (!isIndex(value.activeRow) || !isIndex(value.activeColumn)) return null;
  if (!isSize(value.scrollTop, Number.MAX_SAFE_INTEGER)) return null;
  if (!isSize(value.scrollLeft, Number.MAX_SAFE_INTEGER)) return null;

  let sort: GridLayout['sort'] = null;
  if (isObject(value.sort) && isIndex(value.sort.column) && isDirection(value.sort.direction)) {
    sort = { column: value.sort.column, direction: value.sort.direction };
  }

  return {
    widths,
    heights,
    sort,
    header: typeof value.header === 'boolean' ? value.header : null,
    wrap: typeof value.wrap === 'boolean' ? value.wrap : null,
    fontSize: isSize(value.fontSize, 48) && value.fontSize > 0 ? value.fontSize : null,
    scrollTop: value.scrollTop,
    scrollLeft: value.scrollLeft,
    activeRow: value.activeRow,
    activeColumn: value.activeColumn,
  };
}

function parseEdit(value: unknown): GridEdit | null {
  if (!isObject(value) || typeof value.kind !== 'string') return null;

  switch (value.kind) {
    case 'cells': {
      if (!Array.isArray(value.patches) || value.patches.length > MAX_PATCHES) return null;
      const patches: CellPatch[] = [];
      for (const patch of value.patches) {
        if (!isObject(patch)) return null;
        if (!isIndex(patch.row) || !isIndex(patch.column)) return null;
        if (!isCellValue(patch.value)) return null;
        patches.push({ row: patch.row, column: patch.column, value: patch.value });
      }
      return { kind: 'cells', patches };
    }

    case 'insertRows': {
      if (!isIndex(value.at)) return null;
      if (!Array.isArray(value.rows) || value.rows.length === 0) return null;
      if (value.rows.length > 100_000) return null;
      const rows: string[][] = [];
      let cells = 0;
      for (const row of value.rows) {
        if (!Array.isArray(row)) return null;
        cells += row.length;
        if (cells > MAX_PATCHES) return null;
        if (!row.every(isCellValue)) return null;
        rows.push([...row]);
      }
      return { kind: 'insertRows', at: value.at, rows };
    }

    case 'deleteRows': {
      const rows = parseIndexList(value.rows);
      return rows && rows.length > 0 ? { kind: 'deleteRows', rows } : null;
    }

    case 'insertColumns': {
      if (!isIndex(value.at)) return null;
      if (!isIndex(value.count) || value.count < 1 || value.count > 10_000) return null;
      return { kind: 'insertColumns', at: value.at, count: value.count };
    }

    case 'deleteColumns': {
      const columns = parseIndexList(value.columns);
      return columns && columns.length > 0 ? { kind: 'deleteColumns', columns } : null;
    }

    case 'sort': {
      if (!isIndex(value.column) || !isDirection(value.direction)) return null;
      if (typeof value.header !== 'boolean') return null;
      return {
        kind: 'sort',
        column: value.column,
        direction: value.direction,
        header: value.header,
      };
    }

    default:
      return null;
  }
}

function parseIndexList(value: unknown): number[] | null {
  if (!Array.isArray(value) || value.length > 1_000_000) return null;
  const out: number[] = [];
  for (const entry of value) {
    if (!isIndex(entry)) return null;
    out.push(entry);
  }
  return out;
}

function parsePlace(value: unknown): GridPlace | null {
  if (!isObject(value)) return null;
  if (!isIndex(value.row) || !isIndex(value.column)) return null;
  if (!isIndex(value.rows) || !isIndex(value.columns)) return null;
  if (!isIndex(value.selected)) return null;
  if (typeof value.columnName !== 'string') return null;
  const layout = parseLayout(value.layout);
  if (!layout) return null;
  return {
    row: value.row,
    column: value.column,
    columnName: value.columnName.slice(0, 200),
    rows: value.rows,
    columns: value.columns,
    selected: value.selected,
    layout,
  };
}

/**
 * Validate a message from the webview.
 *
 * One guard per variant, hand-written. Returns null for anything that does not
 * match exactly, which the caller logs and drops.
 */
export function parseWebviewMessage(value: unknown): WebviewToHost | null {
  if (!isObject(value) || typeof value.type !== 'string') return null;

  switch (value.type) {
    case 'ready':
      return { type: 'ready' };

    case 'edit': {
      const edit = parseEdit(value.edit);
      return edit ? { type: 'edit', edit } : null;
    }

    case 'place': {
      const place = parsePlace(value.place);
      return place ? { type: 'place', place } : null;
    }

    case 'editing':
      return typeof value.editing === 'boolean'
        ? { type: 'editing', editing: value.editing }
        : null;

    case 'clipboard':
      // Bounded, because this goes to the system clipboard: a page that asked
      // to put a gigabyte on it would take the window down with it.
      return typeof value.text === 'string' && value.text.length <= 64_000_000
        ? { type: 'clipboard', text: value.text }
        : null;

    case 'requestPaste':
      return { type: 'requestPaste' };

    case 'run':
      return HOST_COMMANDS.includes(value.command as HostCommand)
        ? { type: 'run', command: value.command as HostCommand }
        : null;

    case 'error':
      return typeof value.message === 'string' && typeof value.context === 'string'
        ? {
            type: 'error',
            message: value.message.slice(0, 2000),
            context: value.context.slice(0, 200),
          }
        : null;

    default:
      return null;
  }
}
