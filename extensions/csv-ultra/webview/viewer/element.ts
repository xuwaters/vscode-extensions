import { FASTElement, Updates, css, customElement, observable } from '@microsoft/fast-element';
import { delimiterName, type Dialect } from '../../src/csv/dialect.js';
import { gridOf, parse } from '../../src/csv/parse.js';
import {
  columnLabel,
  looksLikeHeader,
  sortOrder,
  summarize,
  type SortDirection,
} from '../../src/csv/values.js';
import type {
  CellPatch,
  GridCommand,
  GridLayout,
  GridSettings,
  HostCommand,
  HostToWebview,
  WebviewToHost,
} from '../../src/messages.js';
import { fromDelimited, toDelimited } from '../model/clipboard.js';
import { ColumnMetrics, RowMetrics } from '../model/metrics.js';
import {
  addCell,
  atCell,
  cellCount,
  clampCell,
  extendTo,
  reclamp,
  selectAll,
  selectColumns,
  selectRows,
  selectedColumns,
  selectedRows,
  type Cell,
  type Selection,
} from '../model/selection.js';
import { edgeFrom, findMatches, nextMatch } from '../model/view.js';
import { Sheet, type PickTarget, type SheetView } from '../render/sheet.js';
import sheetStyles from './styles.css';
import { template } from './template.js';

/** How long a transient notice stays in the footer. */
const NOTICE_MS = 2600;

/** How often the reader's place is reported to the host, at most. */
const PLACE_MS = 150;

/** How far a keystroke moves the row height or the font size. */
const HEIGHT_STEP = 4;
const FONT_STEP = 1;

/** Most cells one Delete or paste will write. */
const MAX_WRITE = 500_000;

const styles = css`
  ${sheetStyles}
`;

/** What the element needs from the extension host. */
export interface GridHost {
  post(message: WebviewToHost): void;
}

/** One entry of the right-click menu. */
export interface MenuItem {
  label: string;
  run: () => void;
}

/**
 * The tag, named once. `@customElement` in fast-element 3 defines the element
 * *asynchronously* — `compose` resolves a promise before `customElements.define`
 * is ever called — so the bootstrap has to wait on this name rather than
 * construct the class the moment the module has evaluated.
 */
export const CSV_GRID_TAG = 'csv-grid';

/**
 * `<csv-grid>` — the whole table.
 *
 * The split is pdf-ultra's and for the same reason: the *chrome* is reactive and
 * lives in the template, and the *cells* are not. `Sheet` owns a few hundred
 * recycled boxes over a file of any size; this element owns everything the reader
 * can see the state of, and every decision about what a gesture means.
 *
 * Two coordinate systems meet here, and keeping them apart is most of the work:
 *
 * * **View** coordinates are what is on screen — row 0 is the first row under the
 *   header, and a sorted table's row 0 can be any record in the file.
 * * **File** coordinates are records in the document, which is the only thing the
 *   host will accept. `fileRow` is the one crossing, and every edit goes through
 *   it.
 *
 * The table also carries one row and one column that are not in the file: the
 * blank ones past the end. Typing in them is how a table grows, and the host
 * turns a write past the last record into an append — so growing a file is the
 * same gesture as editing it, rather than a menu item.
 */
@customElement({ name: CSV_GRID_TAG, template, styles })
export class CsvGrid extends FASTElement implements SheetView {
  /** Set by the bootstrap before the element is attached. */
  host!: GridHost;

  @observable state: 'idle' | 'ready' | 'refused' = 'idle';
  @observable name = 'table.csv';
  @observable notice = '';
  @observable cardMessage = 'Opening…';

  @observable rowField = '1';
  @observable rowsLabel = '0';
  @observable cellLabel = '';
  @observable summary = '';
  @observable delimiterLabel = 'Comma';
  @observable sortLabel = '';

  @observable findOpen = false;
  @observable findQuery = '';
  @observable findCount = '';
  @observable matchCase = false;
  @observable wholeCell = false;

  @observable sort: { column: number; direction: SortDirection } | null = null;
  @observable menu: { x: number; y: number; items: MenuItem[] } | null = null;

  /*
   * Mirrors rather than getters, because the toolbar binds to them. A FAST
   * binding subscribes to the observables it *reads*, and a getter over plain
   * fields reads none — so the button would go on saying `off` after the flag
   * flipped. `refreshFlags` is the one place either is computed.
   */
  @observable hasHeader = false;
  @observable wrap = false;
  @observable readOnly = false;

  /** Bound by the template. */
  tableEl!: HTMLElement;
  viewportEl!: HTMLElement;
  canvasEl!: HTMLElement;
  columnHeadEl!: HTMLElement;
  rowHeadEl!: HTMLElement;
  measureEl!: HTMLElement;
  findInput!: HTMLInputElement;
  rowInput!: HTMLInputElement;

  readonly columnMetrics = new ColumnMetrics(140);
  readonly rowMetrics = new RowMetrics(24);

  @observable selection: Selection = {
    ranges: [],
    active: { row: 0, column: 0 },
    anchor: { row: 0, column: 0 },
    mode: 'cells',
  };

  matchKeys = new Set<string>();
  activeMatch: Cell | null = null;

  private sheet: Sheet | null = null;
  private settings: GridSettings = {
    headerRow: 'auto',
    rowHeight: 24,
    columnWidth: 140,
    maxColumnWidth: 480,
    autoFitOnOpen: true,
    fontSize: 0,
    fontFamily: 'editor',
    wrap: false,
    zebraStripes: true,
    alignNumbers: true,
    readOnly: false,
  };

  /** Every record's values, padded, in file order. */
  private records: string[][] = [];
  /** View row → file record. Excludes the header row. */
  private order: number[] = [];
  private columnCount = 0;
  private guessedHeader = false;
  private headerOverride: boolean | null = null;
  private wrapOverride: boolean | null = null;
  private fontOverride: number | null = null;
  private matchList: Cell[] = [];
  private matchIndex = -1;
  private editingAt: Cell | null = null;
  private noticeTimer: ReturnType<typeof setTimeout> | undefined;
  private placeTimer: ReturnType<typeof setTimeout> | undefined;
  private resizeObserver: ResizeObserver | null = null;

  // ── The view the sheet draws ──────────────────────────────────────────────

  /**
   * Rows on screen: the file's, plus one blank.
   *
   * The blank row at the bottom is not a placeholder. Typing in it writes past
   * the last record, which the host turns into an append — so a table grows by
   * being typed into, the way a spreadsheet does, rather than by finding a menu.
   */
  get rows(): number {
    return this.order.length + 1;
  }

  /** Columns on screen: the file's, plus one blank, for the same reason. */
  get columns(): number {
    return this.columnCount + 1;
  }

  /** Recompute the flags the toolbar shows and the sheet draws from. */
  private refreshFlags(): void {
    this.hasHeader =
      this.headerOverride ??
      (this.settings.headerRow === 'always'
        ? this.records.length > 0
        : this.settings.headerRow === 'never'
          ? false
          : this.guessedHeader);
    this.wrap = this.wrapOverride ?? this.settings.wrap;
    // No per-tab override, deliberately: read-only is a mode the reader turned
    // on, and a tab that quietly kept writing would defeat the point of it.
    this.readOnly = this.settings.readOnly;
  }

  /**
   * Turn back a gesture that would write, and say why.
   *
   * Read-only is a mode, not a failure, so it answers in the footer where every
   * other thing the table has to say goes — not in a dialog.
   */
  private refuse(): boolean {
    if (!this.readOnly) return false;
    this.say('This table is read-only');
    return true;
  }

  get zebra(): boolean {
    return this.settings.zebraStripes;
  }

  get alignNumbers(): boolean {
    return this.settings.alignNumbers;
  }

  value(row: number, column: number): string {
    const file = this.order[row];
    if (file === undefined) return '';
    return this.records[file]?.[column] ?? '';
  }

  header(column: number): string {
    return this.hasHeader ? (this.records[0]?.[column] ?? '') : '';
  }

  /**
   * The number beside a row: its place in the *file*, not in the view.
   *
   * Which is the useful answer once the view is sorted — the numbers come out
   * shuffled, and that is the point. It says where the row really is, so a row
   * found by sorting can be found again in the text editor.
   */
  rowNumber(row: number): number {
    const file = this.order[row];
    return (file ?? this.records.length) + 1;
  }

  // ── Lifecycle ─────────────────────────────────────────────────────────────

  override connectedCallback(): void {
    super.connectedCallback();
    this.sheet = new Sheet(
      {
        table: this.tableEl,
        viewport: this.viewportEl,
        canvas: this.canvasEl,
        columnHead: this.columnHeadEl,
        rowHead: this.rowHeadEl,
        measure: this.measureEl,
      },
      this,
      {
        onPick: (target, event) => this.onPick(target, event),
        onDragTo: (target) => this.onDragTo(target),
        onDragEnd: () => this.reportPlace(),
        onOpenEditor: (row, column) => this.beginEdit({ row, column }, true),
        onColumnResized: (column, width) => {
          this.columnMetrics.setWidth(column, width);
          this.paint();
        },
        onRowResized: (row, height) => {
          this.rowMetrics.setHeight(row, height);
          this.paint();
        },
        onFitColumn: (column) => this.fitColumns([column]),
        onFitRow: (row) => this.fitRows([row]),
        onSortToggle: (column) => this.cycleSort(column),
        onScrolled: () => this.reportPlace(),
        onContext: (target, event) => this.openMenu(target, event),
      },
    );

    this.sheet.input.addEventListener('keydown', this.onEditorKeydown);
    this.sheet.input.addEventListener('blur', this.onEditorBlur);
    this.addEventListener('pointerdown', this.onAnyPointerDown, true);

    this.resizeObserver = new ResizeObserver(() => this.paint());
    this.resizeObserver.observe(this.tableEl);
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    if (this.noticeTimer) clearTimeout(this.noticeTimer);
    if (this.placeTimer) clearTimeout(this.placeTimer);
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    this.sheet?.input.removeEventListener('keydown', this.onEditorKeydown);
    this.sheet?.input.removeEventListener('blur', this.onEditorBlur);
    this.removeEventListener('pointerdown', this.onAnyPointerDown, true);
    this.sheet?.dispose();
    this.sheet = null;
  }

  // ── Messages from the host ────────────────────────────────────────────────

  handle(message: HostToWebview): void {
    switch (message.type) {
      case 'load':
        this.load(message.name, message.text, message.dialect, message.settings, message.layout);
        break;

      case 'select': {
        const view = this.order.indexOf(message.row);
        this.goTo({ row: view >= 0 ? view : 0, column: message.column }, false);
        break;
      }

      case 'settings':
        this.applySettings(message.settings);
        break;

      case 'command':
        this.command(message.command);
        break;

      case 'paste':
        this.pasteText(message.text);
        break;

      case 'refused':
        this.state = 'refused';
        this.cardMessage =
          `This file is ${megabytes(message.bytes)} — larger than the ` +
          `${megabytes(message.limit)} the table is allowed to build from. ` +
          `Raise csvUltra.maxFileSizeBytes, or read it as text.`;
        break;

      case 'visible':
        this.paint();
        break;

      case 'focus':
        this.focusTable();
        break;

      case 'hostError':
        this.state = 'refused';
        this.cardMessage = message.message;
        break;
    }
  }

  /**
   * Take a document.
   *
   * The whole file, re-parsed, every time — see the session on why there is no
   * incremental path. What survives it is the reader's *place*: the selection,
   * the scroll, the widths and the sort are all re-applied to the new table
   * rather than reset, because most loads are somebody typing in a text editor
   * beside this one and the table should not jump every time they do.
   */
  private load(
    name: string,
    text: string,
    dialect: Dialect,
    settings: GridSettings,
    layout?: GridLayout,
  ): void {
    const first = this.state !== 'ready';
    this.name = name;
    this.delimiterLabel = delimiterName(dialect.delimiter);
    this.settings = settings;

    const table = parse(text, dialect);
    this.records = gridOf(table);
    this.columnCount = table.columns;
    this.guessedHeader = looksLikeHeader(this.records);

    if (first && layout) this.restore(layout);
    this.refreshFlags();
    this.applyMetrics();
    this.rebuildOrder();

    this.state = 'ready';
    if (first && layout) {
      // A parked layout names a *file* row, because a view row means nothing
      // once the sort it was taken under has changed — so it is looked up
      // through the order rather than used as an index.
      const view = this.order.indexOf(layout.activeRow);
      this.selection = atCell(
        { row: view >= 0 ? view : 0, column: layout.activeColumn },
        this.bounds,
      );
    }
    this.selection = reclamp(this.selection, this.bounds);
    if (this.selection.ranges.length === 0) this.selection = atCell({ row: 0, column: 0 }, this.bounds);
    this.refreshFind();
    this.refreshLabels();
    this.sheet?.invalidate();

    void Updates.next().then(() => {
      if (first) {
        if (layout) {
          this.viewportEl.scrollTop = layout.scrollTop;
          this.viewportEl.scrollLeft = layout.scrollLeft;
        }
        if (settings.autoFitOnOpen && (layout?.widths.length ?? 0) === 0) {
          this.fitColumns(range(this.columnCount));
        }
        this.focusTable();
      }
      this.paint();
      this.reportPlace(true);
    });
  }

  private restore(layout: GridLayout): void {
    this.columnMetrics.restore(layout.widths);
    this.rowMetrics.restore(layout.heights);
    this.sort = layout.sort;
    this.headerOverride = layout.header;
    this.wrapOverride = layout.wrap;
    this.fontOverride = layout.fontSize;
  }

  private applySettings(settings: GridSettings): void {
    this.settings = settings;
    this.refreshFlags();
    // Locking the table with a cell editor open: the edit in it was never
    // committed, and leaving the box there would only end in a refusal.
    if (this.readOnly && this.sheet?.isEditing) this.cancelEdit();
    this.applyMetrics();
    this.rebuildOrder();
    this.sheet?.invalidate();
    this.paint();
  }

  private applyMetrics(): void {
    this.columnMetrics.setDefault(this.settings.columnWidth);
    this.rowMetrics.setDefault(this.settings.rowHeight);
    const size =
      this.fontOverride ??
      (this.settings.fontSize > 0 ? this.settings.fontSize : cssFallbackSize());
    this.style.setProperty('--cell-size', `${size}px`);
    this.style.setProperty(
      '--cell-font',
      this.settings.fontFamily === 'ui'
        ? 'var(--vscode-font-family)'
        : 'var(--vscode-editor-font-family, monospace)',
    );
    const digits = String(Math.max(1, this.records.length)).length;
    this.style.setProperty('--rowhead-width', `${Math.max(46, 18 + digits * 9)}px`);
  }

  /** The view's row order: the file's, or the sort's. */
  private rebuildOrder(): void {
    const base = this.hasHeader && this.records.length > 0 ? 1 : 0;
    const body = this.records.length - base;
    if (body <= 0) {
      this.order = [];
    } else if (!this.sort) {
      this.order = Array.from({ length: body }, (_, index) => index + base);
    } else {
      const column = this.sort.column;
      const keys = this.records.slice(base).map((row) => row[column] ?? '');
      // The same `sortOrder` the host writes a sort with, so what a reader sees
      // and what "Write" puts in the file cannot come out different.
      this.order = sortOrder(keys, this.sort.direction, 0).map((index) => index + base);
    }
    this.sortLabel = this.sort
      ? this.header(this.sort.column) || columnLabel(this.sort.column)
      : '';
    this.rowsLabel = this.records.length.toLocaleString();
  }

  private get bounds(): { rows: number; columns: number } {
    return { rows: this.rows, columns: this.columns };
  }

  /**
   * A view row as the file numbers it.
   *
   * The rows past the end of `order` are the blank ones the table grows into.
   * They number on from the end of the file, so a block pasted over the bottom
   * edge becomes consecutive new records rather than piling into one.
   *
   * `end` is the end of the file to count from. A caller part way through
   * appending records has to pass the end it *started* with — read afresh, it
   * moves out from under every row still to be written, and a row of cells walks
   * down a diagonal, a record per cell.
   */
  private fileRow(row: number, end = this.records.length): number {
    return this.order[row] ?? end + (row - this.order.length);
  }

  // ── Painting and reporting ────────────────────────────────────────────────

  private paint(): void {
    this.sheet?.schedule();
  }

  /** Tell the host where the reader is — throttled; it drives the status bar. */
  private reportPlace(now = false): void {
    if (this.placeTimer) {
      if (!now) return;
      clearTimeout(this.placeTimer);
      this.placeTimer = undefined;
    }
    const send = (): void => {
      this.placeTimer = undefined;
      if (this.state !== 'ready') return;
      const { active } = this.selection;
      const selected = cellCount(this.selection);
      this.host.post({
        type: 'place',
        place: {
          row: this.rowNumber(active.row),
          column: active.column,
          columnName: this.header(active.column),
          rows: this.records.length,
          columns: this.columnCount,
          selected,
          layout: this.layout(),
        },
      });
    };
    if (now) send();
    else this.placeTimer = setTimeout(send, PLACE_MS);
  }

  private layout(): GridLayout {
    return {
      widths: this.columnMetrics.sizes(),
      heights: this.rowMetrics.sizes(),
      sort: this.sort,
      header: this.headerOverride,
      wrap: this.wrapOverride,
      fontSize: this.fontOverride,
      scrollTop: Math.round(this.viewportEl?.scrollTop ?? 0),
      scrollLeft: Math.round(this.viewportEl?.scrollLeft ?? 0),
      activeRow: this.fileRow(this.selection.active.row),
      activeColumn: this.selection.active.column,
    };
  }

  /** Everything under the selection, for the footer. */
  private refreshLabels(): void {
    const { active } = this.selection;
    this.cellLabel = `${columnLabel(active.column)}${this.rowNumber(active.row)}`;
    this.rowField = String(this.rowNumber(active.row));

    const values: string[] = [];
    let budget = 200_000;
    for (const rect of this.selection.ranges) {
      for (let row = rect.top; row <= rect.bottom && budget > 0; row += 1) {
        for (let column = rect.left; column <= rect.right && budget > 0; column += 1) {
          values.push(this.value(row, column));
          budget -= 1;
        }
      }
    }
    const stats = summarize(values);
    if (stats.cells <= 1) {
      this.summary = '';
      return;
    }
    const parts = [`${stats.cells.toLocaleString()} cells`, `${stats.filled.toLocaleString()} filled`];
    if (stats.numeric > 0) {
      parts.push(
        `sum ${format(stats.sum)}`,
        `avg ${format(stats.sum / stats.numeric)}`,
        `min ${format(stats.min)}`,
        `max ${format(stats.max)}`,
      );
    }
    this.summary = parts.join('  ·  ');
  }

  private setSelection(selection: Selection, reveal = true): void {
    this.selection = selection;
    this.refreshLabels();
    if (reveal) this.sheet?.reveal(selection.active);
    this.paint();
    this.reportPlace();
  }

  private say(message: string): void {
    this.notice = message;
    if (this.noticeTimer) clearTimeout(this.noticeTimer);
    this.noticeTimer = setTimeout(() => {
      this.notice = '';
      this.noticeTimer = undefined;
    }, NOTICE_MS);
  }

  private focusTable(): void {
    if (this.state !== 'ready') return;
    if (this.sheet?.isEditing) return;
    this.tableEl?.focus({ preventScroll: true });
  }

  // ── Pointer ───────────────────────────────────────────────────────────────

  private onPick(target: PickTarget, event: PointerEvent): void {
    this.closeMenu();
    if (this.sheet?.isEditing) this.commitEdit();
    const additive = event.ctrlKey || event.metaKey;
    const extending = event.shiftKey;

    if (target.kind === 'all') {
      this.setSelection(selectAll(this.selection, this.bounds), false);
    } else if (target.kind === 'row') {
      this.setSelection(
        extending
          ? selectRows(this.selection, this.selection.anchor.row, target.row, this.bounds)
          : selectRows(this.selection, target.row, target.row, this.bounds, additive),
        false,
      );
    } else if (target.kind === 'column') {
      this.setSelection(
        extending
          ? selectColumns(this.selection, this.selection.anchor.column, target.column, this.bounds)
          : selectColumns(this.selection, target.column, target.column, this.bounds, additive),
        false,
      );
    } else {
      const cell = { row: target.row, column: target.column };
      if (extending) this.setSelection(extendTo(this.selection, cell, this.bounds), false);
      else if (additive) this.setSelection(addCell(this.selection, cell, this.bounds), false);
      else this.setSelection(atCell(cell, this.bounds), false);
    }
    this.focusTable();
  }

  private onDragTo(target: PickTarget): void {
    if (target.kind === 'cell') {
      this.setSelection(
        extendTo(this.selection, { row: target.row, column: target.column }, this.bounds),
        true,
      );
    } else if (target.kind === 'row') {
      this.setSelection(
        selectRows(this.selection, this.selection.anchor.row, target.row, this.bounds),
        true,
      );
    } else if (target.kind === 'column') {
      this.setSelection(
        selectColumns(this.selection, this.selection.anchor.column, target.column, this.bounds),
        true,
      );
    }
  }

  /** Any click outside the menu dismisses it, the way a menu should. */
  private readonly onAnyPointerDown = (event: Event): void => {
    if (!this.menu) return;
    const path = event.composedPath();
    if (path.some((node) => node instanceof HTMLElement && node.classList.contains('menu'))) return;
    this.closeMenu();
  };

  // ── Keyboard ──────────────────────────────────────────────────────────────

  /**
   * The keys that move around the table.
   *
   * Handled in the page rather than contributed as keybindings, because they are
   * the keys a text editor also uses and contributing them would take the arrow
   * keys away from every other editor in the window whenever a table tab happened
   * to be active. The shortcuts that *are* contributed — find, go to row, the
   * font size — arrive as `command` messages instead, which is also what makes
   * them rebindable.
   *
   * Every key this handler acts on cancels itself, and the ones it does not act
   * on are left strictly alone — the cell editor is drawn inside the table, so
   * a keystroke arriving here on its way up from the editor is a character
   * somebody is typing. See `keys` in the template for the other half of that.
   */
  onKeydown(event: KeyboardEvent): void {
    if (this.state !== 'ready' || this.sheet?.isEditing) return;
    const modifier = event.ctrlKey || event.metaKey;
    const { active } = this.selection;

    const move = (row: number, column: number): void => {
      const cell = clampCell({ row, column }, this.bounds);
      this.setSelection(event.shiftKey ? extendTo(this.selection, cell, this.bounds) : atCell(cell, this.bounds));
      event.preventDefault();
    };

    switch (event.key) {
      case 'ArrowUp':
      case 'ArrowDown':
      case 'ArrowLeft':
      case 'ArrowRight': {
        const deltaRow = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
        const deltaColumn = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
        if (modifier) {
          const edge = edgeFrom(
            (row, column) => this.value(row, column),
            this.rows,
            this.columns,
            active,
            deltaRow,
            deltaColumn,
          );
          move(edge.row, edge.column);
        } else {
          move(active.row + deltaRow, active.column + deltaColumn);
        }
        return;
      }

      case 'Tab':
        // Tab is a move, not a focus change: it is the fastest way along a row
        // and the reason a spreadsheet is quick to type into.
        this.setSelection(
          atCell({ row: active.row, column: active.column + (event.shiftKey ? -1 : 1) }, this.bounds),
        );
        event.preventDefault();
        return;

      case 'Enter':
        if (event.altKey) return;
        this.beginEdit(active, true);
        event.preventDefault();
        return;

      case 'F2':
        this.beginEdit(active, false);
        event.preventDefault();
        return;

      case 'Escape':
        if (this.menu) this.closeMenu();
        else if (this.findOpen) this.closeFind();
        else this.setSelection(atCell(active, this.bounds), false);
        event.preventDefault();
        return;

      case 'Delete':
      case 'Backspace':
        this.clearSelection();
        event.preventDefault();
        return;

      case 'PageUp':
      case 'PageDown': {
        const step = (this.sheet?.pageRows ?? 20) * (event.key === 'PageDown' ? 1 : -1);
        move(active.row + step, active.column);
        return;
      }

      case 'Home':
        move(modifier ? 0 : active.row, 0);
        return;

      case 'End':
        move(modifier ? this.rows - 1 : active.row, this.columns - 1);
        return;

      case 'a':
      case 'A':
        if (!modifier) break;
        this.setSelection(selectAll(this.selection, this.bounds), false);
        event.preventDefault();
        return;

      case 'c':
      case 'C':
        if (!modifier) break;
        this.copy();
        event.preventDefault();
        return;

      case 'x':
      case 'X':
        if (!modifier) break;
        this.copy();
        this.clearSelection();
        event.preventDefault();
        return;

      case 'v':
      case 'V':
        if (!modifier) break;
        this.host.post({ type: 'requestPaste' });
        event.preventDefault();
        return;

      default:
        break;
    }

    // Typing over a cell replaces it, which is how a spreadsheet works and how
    // anybody correcting a column of values expects it to.
    if (!modifier && !event.altKey && event.key.length === 1) {
      this.beginEdit(active, true, event.key);
      event.preventDefault();
    }
  }

  /**
   * The keys the cell editor answers to.
   *
   * The three it consumes stop here rather than being left to bubble. The
   * editor is drawn *inside* the table, so an uncaught keystroke reaches the
   * table's own handler — and by the time Enter gets there the edit it just
   * committed is over, so the table reads it as "open an editor" and puts a
   * fresh one on the cell below. The reader commits one cell and finds
   * themselves editing the next.
   *
   * Everything else is left alone all the way up: the characters are the
   * textarea's, and VSCode watches the same events for `⌘S` and its friends.
   */
  private readonly onEditorKeydown = (event: KeyboardEvent): void => {
    const input = this.sheet?.input;
    if (!input) return;
    const { active } = this.selection;

    if (event.key === 'Escape') {
      this.cancelEdit();
      event.stopPropagation();
      event.preventDefault();
      return;
    }
    if (event.key === 'Enter') {
      event.stopPropagation();
      if (event.altKey) {
        // Alt+Enter puts a line break *in the cell* — which the writer quotes,
        // so a multi-line value stays one field.
        const at = input.selectionStart ?? input.value.length;
        input.value = `${input.value.slice(0, at)}\n${input.value.slice(input.selectionEnd ?? at)}`;
        input.setSelectionRange(at + 1, at + 1);
        event.preventDefault();
        return;
      }
      this.commitEdit();
      this.setSelection(
        atCell({ row: active.row + (event.shiftKey ? -1 : 1), column: active.column }, this.bounds),
      );
      event.preventDefault();
      return;
    }
    if (event.key === 'Tab') {
      event.stopPropagation();
      this.commitEdit();
      this.setSelection(
        atCell({ row: active.row, column: active.column + (event.shiftKey ? -1 : 1) }, this.bounds),
      );
      event.preventDefault();
    }
  };

  /** Clicking away commits, the way leaving a cell in a spreadsheet does. */
  private readonly onEditorBlur = (): void => {
    if (this.sheet?.isEditing) this.commitEdit();
  };

  // ── Editing ───────────────────────────────────────────────────────────────

  private beginEdit(cell: Cell, replace: boolean, seed?: string): void {
    if (this.state !== 'ready' || !this.sheet) return;
    if (this.refuse()) return;
    const at = clampCell(cell, this.bounds);
    this.editingAt = at;
    const current = this.value(at.row, at.column);
    this.sheet.beginEdit(at, seed ?? current, replace && seed === undefined);
    this.host.post({ type: 'editing', editing: true });
  }

  private commitEdit(): void {
    const at = this.editingAt;
    if (!at || !this.sheet) return;
    const value = this.sheet.endEdit();
    this.editingAt = null;
    this.host.post({ type: 'editing', editing: false });
    if (value !== this.value(at.row, at.column)) this.write([{ cell: at, value }]);
    this.focusTable();
  }

  private cancelEdit(): void {
    if (!this.editingAt || !this.sheet) return;
    this.sheet.endEdit();
    this.editingAt = null;
    this.host.post({ type: 'editing', editing: false });
    this.focusTable();
  }

  /**
   * Write cells: on screen at once, and to the document through the host.
   *
   * The local write is what makes typing feel instant — the round trip to the
   * extension host, through a `WorkspaceEdit`, and back would be visible on
   * every keystroke. It is safe because the host is still the only writer: if
   * the edit is refused, or lands differently, the reload that follows replaces
   * this guess with the truth.
   */
  private write(patches: ReadonlyArray<{ cell: Cell; value: string }>): void {
    if (patches.length === 0) return;
    // The backstop, under every gesture that writes cells. The callers refuse
    // first, so they can say something better than "no" about what they were
    // asked to do; this one is here so a path added later cannot slip past.
    if (this.refuse()) return;
    const wire: CellPatch[] = [];
    let grew = false;
    // Where the file ends, taken once: the loop below appends to `records`, and
    // every row of this write has to be numbered against the file as it was.
    const end = this.records.length;

    for (const { cell, value } of patches) {
      const file = this.fileRow(cell.row, end);
      wire.push({ row: file, column: cell.column, value });

      while (this.records.length <= file) {
        this.records.push([]);
        grew = true;
      }
      const record = this.records[file]!;
      while (record.length <= cell.column) record.push('');
      record[cell.column] = value;
      if (cell.column + 1 > this.columnCount) {
        this.columnCount = cell.column + 1;
        grew = true;
      }
    }

    if (grew) {
      this.applyMetrics();
      this.rebuildOrder();
      this.selection = reclamp(this.selection, this.bounds);
    }
    this.host.post({ type: 'edit', edit: { kind: 'cells', patches: wire } });
    this.refreshFind();
    this.refreshLabels();
    this.sheet?.invalidate();
    this.paint();
    this.reportPlace();
  }

  private clearSelection(): void {
    if (this.refuse()) return;
    const patches: Array<{ cell: Cell; value: string }> = [];
    for (const rect of this.selection.ranges) {
      for (let row = rect.top; row <= rect.bottom; row += 1) {
        for (let column = rect.left; column <= rect.right; column += 1) {
          if (this.value(row, column) === '') continue;
          patches.push({ cell: { row, column }, value: '' });
          if (patches.length >= MAX_WRITE) break;
        }
      }
    }
    this.write(patches);
  }

  // ── Clipboard ─────────────────────────────────────────────────────────────

  private copy(): void {
    const text = toDelimited((row, column) => this.value(row, column), this.selection);
    this.host.post({ type: 'clipboard', text });
    this.say(`Copied ${cellCount(this.selection).toLocaleString()} cells`);
  }

  private pasteText(text: string): void {
    if (this.refuse()) return;
    const block = fromDelimited(text);
    if (block.length === 0) return;
    const { active } = this.selection;
    const patches: Array<{ cell: Cell; value: string }> = [];

    for (let row = 0; row < block.length; row += 1) {
      const line = block[row]!;
      for (let column = 0; column < line.length; column += 1) {
        patches.push({
          cell: { row: active.row + row, column: active.column + column },
          value: line[column] ?? '',
        });
        if (patches.length >= MAX_WRITE) break;
      }
    }
    this.write(patches);

    const bottom = active.row + block.length - 1;
    const right = active.column + Math.max(...block.map((line) => line.length)) - 1;
    this.setSelection(
      extendTo(atCell(active, this.bounds), { row: bottom, column: right }, this.bounds),
    );
    this.say(`Pasted ${block.length}×${Math.max(...block.map((line) => line.length))}`);
  }

  // ── Structure ─────────────────────────────────────────────────────────────

  private insertRows(below: boolean): void {
    if (this.refuse()) return;
    const rows = selectedRows(this.selection).filter((row) => row < this.order.length);
    const anchor = rows.length > 0 ? (below ? rows[rows.length - 1]! : rows[0]!) : this.selection.active.row;
    const at = this.fileRow(anchor) + (below ? 1 : 0);
    const count = Math.max(1, rows.length);
    this.host.post({
      type: 'edit',
      edit: {
        kind: 'insertRows',
        at,
        rows: Array.from({ length: count }, () => ['']),
      },
    });
    this.say(count === 1 ? 'Inserted a row' : `Inserted ${count} rows`);
  }

  private deleteRows(): void {
    if (this.refuse()) return;
    const rows = selectedRows(this.selection)
      .filter((row) => row < this.order.length)
      .map((row) => this.fileRow(row));
    if (rows.length === 0) return this.say('No rows selected');
    // The header is a label, not a row; deleting it from a row selection that
    // swept the whole table is never what was meant.
    this.host.post({ type: 'edit', edit: { kind: 'deleteRows', rows } });
    this.say(rows.length === 1 ? 'Deleted a row' : `Deleted ${rows.length} rows`);
  }

  private insertColumns(right: boolean): void {
    if (this.refuse()) return;
    const columns = selectedColumns(this.selection);
    const anchor = columns.length > 0 ? (right ? columns[columns.length - 1]! : columns[0]!) : this.selection.active.column;
    const count = Math.max(1, columns.length);
    this.host.post({
      type: 'edit',
      edit: { kind: 'insertColumns', at: anchor + (right ? 1 : 0), count },
    });
    this.say(count === 1 ? 'Inserted a column' : `Inserted ${count} columns`);
  }

  private deleteColumns(): void {
    if (this.refuse()) return;
    const columns = selectedColumns(this.selection).filter((column) => column < this.columnCount);
    if (columns.length === 0) return this.say('No columns selected');
    this.host.post({ type: 'edit', edit: { kind: 'deleteColumns', columns } });
    this.say(columns.length === 1 ? 'Deleted a column' : `Deleted ${columns.length} columns`);
  }

  // ── Sorting ───────────────────────────────────────────────────────────────

  /**
   * Ascending, then descending, then back to the file's own order.
   *
   * A sort here is a *view*, not a change to the file — nothing is written and
   * nothing is dirty until the reader asks for it with Write. Which is the right
   * default for a format whose row order is often the data: a log, a ledger, an
   * export with a meaningful sequence.
   */
  private cycleSort(column: number): void {
    if (column >= this.columnCount) return;
    if (this.sort?.column !== column) this.setSort({ column, direction: 'asc' });
    else if (this.sort.direction === 'asc') this.setSort({ column, direction: 'desc' });
    else this.setSort(null);
  }

  private setSort(sort: { column: number; direction: SortDirection } | null): void {
    this.sort = sort;
    this.rebuildOrder();
    this.selection = reclamp(this.selection, this.bounds);
    this.refreshFind();
    this.refreshLabels();
    this.sheet?.invalidate();
    this.paint();
    this.reportPlace();
  }

  /** The template's Write button, and the palette's command. */
  applySort(): void {
    if (this.refuse()) return;
    if (!this.sort) return this.say('Nothing is sorted');
    this.host.post({
      type: 'edit',
      edit: {
        kind: 'sort',
        column: this.sort.column,
        direction: this.sort.direction,
        header: this.hasHeader,
      },
    });
    this.sort = null;
    this.say('Wrote the sorted order into the file');
  }

  clearSort(): void {
    this.setSort(null);
  }

  // ── Find ──────────────────────────────────────────────────────────────────

  openFind(): void {
    this.findOpen = true;
    void Updates.next().then(() => {
      this.findInput?.focus();
      this.findInput?.select();
    });
  }

  closeFind(): void {
    this.findOpen = false;
    this.findQuery = '';
    this.refreshFind();
    this.paint();
    this.focusTable();
  }

  onFindInput(event: Event): void {
    this.findQuery = (event.target as HTMLInputElement).value;
    this.refreshFind();
    if (this.matchList.length > 0) this.jumpTo(0);
    this.paint();
  }

  onFindKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      this.step(!event.shiftKey);
      event.preventDefault();
    } else if (event.key === 'Escape') {
      this.closeFind();
      event.preventDefault();
    }
  }

  toggleMatchCase(): void {
    this.matchCase = !this.matchCase;
    this.refreshFind();
    this.paint();
  }

  toggleWholeCell(): void {
    this.wholeCell = !this.wholeCell;
    this.refreshFind();
    this.paint();
  }

  step(forward: boolean): void {
    if (this.matchList.length === 0) return;
    const index = nextMatch(this.matchList, this.selection.active, forward);
    if (index >= 0) this.jumpTo(index);
  }

  private jumpTo(index: number): void {
    const match = this.matchList[index];
    if (!match) return;
    this.matchIndex = index;
    this.activeMatch = match;
    this.findCount = `${index + 1} of ${this.matchList.length.toLocaleString()}`;
    this.setSelection(atCell(match, this.bounds));
  }

  private refreshFind(): void {
    if (this.findQuery === '') {
      this.matchList = [];
      this.matchKeys = new Set();
      this.activeMatch = null;
      this.matchIndex = -1;
      this.findCount = '';
      return;
    }
    this.matchList = findMatches(
      (row, column) => this.value(row, column),
      this.order.length,
      this.columnCount,
      { query: this.findQuery, matchCase: this.matchCase, wholeCell: this.wholeCell },
    );
    this.matchKeys = new Set(this.matchList.map((cell) => `${cell.row}:${cell.column}`));
    this.matchIndex = Math.min(this.matchIndex, this.matchList.length - 1);
    this.activeMatch = this.matchList[this.matchIndex] ?? null;
    this.findCount =
      this.matchList.length === 0
        ? 'No results'
        : `${Math.max(1, this.matchIndex + 1)} of ${this.matchList.length.toLocaleString()}`;
  }

  // ── Layout ────────────────────────────────────────────────────────────────

  toggleHeader(): void {
    this.headerOverride = !this.hasHeader;
    this.refreshFlags();
    this.applyMetrics();
    this.rebuildOrder();
    this.selection = reclamp(this.selection, this.bounds);
    this.sheet?.invalidate();
    this.paint();
    this.reportPlace();
  }

  toggleWrap(): void {
    this.wrapOverride = !this.wrap;
    this.refreshFlags();
    this.sheet?.invalidate();
    this.paint();
    this.reportPlace();
  }

  autoFitColumns(): void {
    const columns = selectedColumns(this.selection).filter((column) => column < this.columnCount);
    this.fitColumns(columns.length > 1 ? columns : range(this.columnCount));
  }

  private fitColumns(columns: readonly number[]): void {
    if (!this.sheet || columns.length === 0) return;
    for (const column of columns) {
      this.columnMetrics.setWidth(column, this.sheet.fitColumn(column, this.settings.maxColumnWidth));
    }
    this.paint();
    this.reportPlace();
  }

  private fitRows(rows: readonly number[]): void {
    if (!this.sheet) return;
    for (const row of rows) {
      this.rowMetrics.setHeight(row, this.sheet.fitRow(row, this.rowMetrics.max));
    }
    this.paint();
    this.reportPlace();
  }

  private resetLayout(): void {
    this.columnMetrics.clear();
    this.rowMetrics.clear();
    this.fontOverride = null;
    this.applyMetrics();
    this.paint();
    this.reportPlace();
    this.say('Reset the widths and heights');
  }

  private stepRowHeight(delta: number): void {
    this.rowMetrics.setDefault(this.rowMetrics.defaultHeight + delta);
    this.paint();
    this.reportPlace();
  }

  private stepFont(delta: number | null): void {
    if (delta === null) this.fontOverride = null;
    else {
      const current = this.fontOverride ?? this.settings.fontSize;
      this.fontOverride = Math.min(40, Math.max(6, (current || cssFallbackSize()) + delta));
    }
    this.applyMetrics();
    this.paint();
    this.reportPlace();
  }

  // ── The row box ───────────────────────────────────────────────────────────

  onRowEntered(event: Event): void {
    const wanted = Number.parseInt((event.target as HTMLInputElement).value.trim(), 10);
    if (!Number.isFinite(wanted)) return this.showRow();
    // The box takes a *file* row, which is what its numbers say — so a row found
    // in the text editor can be typed straight in, sorted view or not.
    const view = this.order.indexOf(Math.max(0, wanted - 1));
    this.goTo(
      { row: view >= 0 ? view : Math.min(this.rows - 1, Math.max(0, wanted - 1)), column: this.selection.active.column },
      true,
    );
  }

  onRowKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      this.showRow();
      this.focusTable();
      event.preventDefault();
    }
  }

  showRow(): void {
    this.rowField = String(this.rowNumber(this.selection.active.row));
  }

  private goTo(cell: Cell, thenFocus: boolean): void {
    this.setSelection(atCell(cell, this.bounds));
    if (thenFocus) this.focusTable();
  }

  // ── The right-click menu ──────────────────────────────────────────────────

  private openMenu(target: PickTarget, event: MouseEvent): void {
    if (target.kind === 'cell') {
      const cell = { row: target.row, column: target.column };
      const inside = this.selection.ranges.some(
        (rect) =>
          cell.row >= rect.top &&
          cell.row <= rect.bottom &&
          cell.column >= rect.left &&
          cell.column <= rect.right,
      );
      // Right-clicking outside the selection moves it there first, which is what
      // every table does and what stops a menu acting on something off screen.
      if (!inside) this.setSelection(atCell(cell, this.bounds), false);
    } else if (target.kind === 'row') {
      this.setSelection(selectRows(this.selection, target.row, target.row, this.bounds), false);
    } else if (target.kind === 'column') {
      this.setSelection(
        selectColumns(this.selection, target.column, target.column, this.bounds),
        false,
      );
    }

    const rect = this.getBoundingClientRect();
    this.menu = {
      x: event.clientX - rect.left,
      y: event.clientY - rect.top,
      items: this.menuFor(target),
    };
  }

  private menuFor(target: PickTarget): MenuItem[] {
    const items: MenuItem[] = [{ label: 'Copy', run: () => this.copy() }];
    // A read-only menu is the short one. Offering Delete rows and answering
    // "no" would be a menu that lies about what the table will do.
    if (this.readOnly) {
      if (target.kind === 'column') {
        items.push(
          { label: 'Sort ascending', run: () => this.setSort({ column: target.column, direction: 'asc' }) },
          { label: 'Sort descending', run: () => this.setSort({ column: target.column, direction: 'desc' }) },
          { label: 'Fit width', run: () => this.fitColumns([target.column]) },
        );
      }
      if (target.kind === 'row') items.push({ label: 'Fit height', run: () => this.fitRows([target.row]) });
      return items;
    }
    items.push(
      { label: 'Paste', run: () => this.host.post({ type: 'requestPaste' }) },
      { label: 'Clear', run: () => this.clearSelection() },
    );
    if (target.kind !== 'column') {
      items.push(
        { label: 'Insert row above', run: () => this.insertRows(false) },
        { label: 'Insert row below', run: () => this.insertRows(true) },
        { label: 'Delete rows', run: () => this.deleteRows() },
      );
    }
    if (target.kind !== 'row') {
      items.push(
        { label: 'Insert column left', run: () => this.insertColumns(false) },
        { label: 'Insert column right', run: () => this.insertColumns(true) },
        { label: 'Delete columns', run: () => this.deleteColumns() },
      );
    }
    if (target.kind === 'column') {
      items.push(
        { label: 'Sort ascending', run: () => this.setSort({ column: target.column, direction: 'asc' }) },
        { label: 'Sort descending', run: () => this.setSort({ column: target.column, direction: 'desc' }) },
        { label: 'Fit width', run: () => this.fitColumns([target.column]) },
      );
    }
    if (target.kind === 'row') {
      items.push({ label: 'Fit height', run: () => this.fitRows([target.row]) });
    }
    return items;
  }

  /** Run a menu item and put it away. Bound by the template. */
  pick(item: MenuItem): void {
    this.closeMenu();
    item.run();
  }

  closeMenu(): void {
    this.menu = null;
  }

  // ── Commands from the host ────────────────────────────────────────────────

  /** Ask the host for something only it can do — a dialog, or the editor swap. */
  runHost(command: HostCommand): void {
    this.host.post({ type: 'run', command });
  }

  private command(command: GridCommand): void {
    if (this.state !== 'ready' && command !== 'toggleHeaderRow') return;
    switch (command) {
      case 'find':
        this.openFind();
        return;
      case 'findNext':
        this.step(true);
        return;
      case 'findPrevious':
        this.step(false);
        return;
      case 'goToRow':
        this.rowInput?.focus();
        this.rowInput?.select();
        return;
      case 'toggleHeaderRow':
        this.toggleHeader();
        return;
      case 'sortAscending':
        this.setSort({ column: this.selection.active.column, direction: 'asc' });
        return;
      case 'sortDescending':
        this.setSort({ column: this.selection.active.column, direction: 'desc' });
        return;
      case 'clearSort':
        this.clearSort();
        return;
      case 'applySort':
        this.applySort();
        return;
      case 'insertRowAbove':
        this.insertRows(false);
        return;
      case 'insertRowBelow':
        this.insertRows(true);
        return;
      case 'deleteRows':
        this.deleteRows();
        return;
      case 'insertColumnLeft':
        this.insertColumns(false);
        return;
      case 'insertColumnRight':
        this.insertColumns(true);
        return;
      case 'deleteColumns':
        this.deleteColumns();
        return;
      case 'toggleWrap':
        this.toggleWrap();
        return;
      case 'autoFitColumns':
        this.autoFitColumns();
        return;
      case 'resetLayout':
        this.resetLayout();
        return;
      case 'rowHeightIncrease':
        this.stepRowHeight(HEIGHT_STEP);
        return;
      case 'rowHeightDecrease':
        this.stepRowHeight(-HEIGHT_STEP);
        return;
      case 'fontSizeIncrease':
        this.stepFont(FONT_STEP);
        return;
      case 'fontSizeDecrease':
        this.stepFont(-FONT_STEP);
        return;
      case 'fontSizeReset':
        this.stepFont(null);
        return;
    }
  }
}

function range(count: number): number[] {
  return Array.from({ length: count }, (_, index) => index);
}

/** The editor's font size, when nothing has overridden it. */
function cssFallbackSize(): number {
  if (typeof getComputedStyle !== 'function') return 13;
  const value = getComputedStyle(document.documentElement).getPropertyValue(
    '--vscode-editor-font-size',
  );
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 13;
}

function format(value: number): string {
  if (!Number.isFinite(value)) return '—';
  const rounded = Math.round(value * 1000) / 1000;
  return rounded.toLocaleString(undefined, { maximumFractionDigits: 3 });
}

function megabytes(bytes: number): string {
  return `${(bytes / (1 << 20)).toFixed(1)} MB`;
}
