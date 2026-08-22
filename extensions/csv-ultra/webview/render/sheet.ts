import { columnLabel, parseNumber } from '../../src/csv/values.js';
import type { SortDirection } from '../../src/messages.js';
import type { ColumnMetrics, RowMetrics } from '../model/metrics.js';
import { contains, hasColumn, hasRow, touchesColumn, touchesRow, type Cell, type Selection } from '../model/selection.js';

/** How wide the drag zone on a header's edge is. */
const HANDLE = 6;

/** Padding inside a cell, both sides, when a column is fitted to its content. */
const FIT_PADDING = 18;

/** Rows sampled below the fold when a column is fitted. */
const FIT_SAMPLE = 300;

/** What the sheet needs to know to draw itself. Implemented by `<csv-grid>`. */
export interface SheetView {
  rows: number;
  columns: number;
  value(row: number, column: number): string;
  /** The column's title, or '' when the file has no header row. */
  header(column: number): string;
  /** The row's number as the reader counts it — the *file* row, sort or no sort. */
  rowNumber(row: number): number;
  columnMetrics: ColumnMetrics;
  rowMetrics: RowMetrics;
  selection: Selection;
  /**
   * `row:column` of every find match.
   *
   * Not `matches`: the element implementing this interface is an `HTMLElement`,
   * which already has a `matches()` of its own.
   */
  matchKeys: Set<string>;
  activeMatch: Cell | null;
  wrap: boolean;
  zebra: boolean;
  alignNumbers: boolean;
  hasHeader: boolean;
  sort: { column: number; direction: SortDirection } | null;
}

/** What a gesture on the sheet means. The element decides; the sheet only reports. */
export interface SheetCallbacks {
  onPick(target: PickTarget, event: PointerEvent): void;
  onDragTo(target: PickTarget): void;
  onDragEnd(): void;
  onOpenEditor(row: number, column: number): void;
  /**
   * A column's edge is being dragged.
   *
   * The phase is reported as well as the size because what a drag *means* can
   * take more than one column with it, and the element wants to work that out
   * once, on the press, rather than on every frame of the gesture.
   *
   * `start` carries the size the column already had and is not a change; `move`
   * is the size under the pointer now; `end` closes the gesture and carries the
   * size it closed at.
   */
  onColumnResized(column: number, width: number, phase: ResizePhase): void;
  onRowResized(row: number, height: number, phase: ResizePhase): void;
  onFitColumn(column: number): void;
  onFitRow(row: number): void;
  onSortToggle(column: number): void;
  onScrolled(): void;
  onContext(target: PickTarget, event: MouseEvent): void;
}

/** Where a resize drag has got to: the press, a move, the release. */
export type ResizePhase = 'start' | 'move' | 'end';

/** What was under the pointer. */
export type PickTarget =
  | { kind: 'cell'; row: number; column: number }
  | { kind: 'row'; row: number }
  | { kind: 'column'; column: number }
  | { kind: 'all' };

/** The elements the template staked out for the sheet to draw into. */
export interface SheetElements {
  table: HTMLElement;
  viewport: HTMLElement;
  canvas: HTMLElement;
  columnHead: HTMLElement;
  rowHead: HTMLElement;
  measure: HTMLElement;
}

/**
 * The scrolling table.
 *
 * Imperative, and deliberately outside the reactive template, for the same
 * reason `PageColumn` is in pdf-ultra: a binding per cell would be two million
 * bindings for a file the reader can see four hundred cells of. What is on screen
 * is a few hundred recycled `div`s absolutely positioned inside one box the size
 * of the whole table, repositioned on every scroll frame from the arithmetic in
 * `metrics.ts`. Nothing below the fold exists.
 *
 * The headers are the other half of it. They are *not* inside the scroller — a
 * sticky header inside a two-axis scroller is a fight with the browser that
 * nobody wins — but overlays beside it, translated by the scroll offset each
 * frame. Column headers slide sideways, row numbers slide up, the corner does
 * neither, and the scrollbars stay where they belong, around the cells.
 *
 * Hit testing is arithmetic rather than `event.target`: the row and column under
 * the pointer come from `indexAt`, the same function that placed the cell there.
 * That way a drag over the gap between two cells, or off the bottom of the last
 * one, still names a cell — which is what a selection drag needs and what a DOM
 * lookup cannot give.
 */
export class Sheet {
  private readonly liveRows = new Map<number, RowElements>();
  private readonly rowPool: HTMLElement[] = [];
  private readonly cellPool: HTMLElement[] = [];
  private readonly liveHeads = new Map<number, HTMLElement>();
  private readonly headPool: HTMLElement[] = [];
  private readonly liveNumbers = new Map<number, HTMLElement>();
  private readonly numberPool: HTMLElement[] = [];
  private readonly overlay: HTMLElement;
  private readonly focusRing: HTMLElement;
  private readonly editor: HTMLTextAreaElement;

  private frame = 0;
  private dragging: Drag | null = null;
  private editingCell: Cell | null = null;
  /** Bumped whenever the data changes, so a repaint knows to refill text. */
  private generation = 0;

  constructor(
    private readonly elements: SheetElements,
    private readonly view: SheetView,
    private readonly callbacks: SheetCallbacks,
  ) {
    this.overlay = document.createElement('div');
    this.overlay.className = 'ranges';
    this.focusRing = document.createElement('div');
    this.focusRing.className = 'focus-ring';
    this.editor = document.createElement('textarea');
    this.editor.className = 'cell-editor';
    this.editor.hidden = true;
    this.editor.spellcheck = false;
    this.editor.wrap = 'off';
    elements.canvas.append(this.overlay, this.focusRing, this.editor);

    elements.viewport.addEventListener('scroll', this.onScroll, { passive: true });
    elements.table.addEventListener('pointerdown', this.onPointerDown);
    elements.table.addEventListener('dblclick', this.onDoubleClick);
    elements.table.addEventListener('contextmenu', this.onContextMenu);
  }

  /** The cell editor, for the element that owns what typing into it means. */
  get input(): HTMLTextAreaElement {
    return this.editor;
  }

  /** Say the data changed, so the next paint refills every visible cell. */
  invalidate(): void {
    this.generation += 1;
  }

  /** Draw, at most once per animation frame. */
  schedule(): void {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.render();
    });
  }

  /** Draw now — for the paths that must not wait, like a reveal before a measure. */
  render(): void {
    const { viewport, canvas, columnHead, rowHead } = this.elements;
    const { columnMetrics, rowMetrics } = this.view;
    columnMetrics.setCount(this.view.columns);
    rowMetrics.setCount(this.view.rows);

    const width = columnMetrics.total();
    const height = rowMetrics.total();
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;

    const { scrollTop, scrollLeft, clientWidth, clientHeight } = viewport;
    const rows = rowMetrics.window(scrollTop, scrollTop + clientHeight);
    const columns = columnMetrics.window(scrollLeft, scrollLeft + clientWidth);

    columnHead.firstElementChild?.setAttribute(
      'style',
      `transform: translateX(${-scrollLeft}px); width: ${width}px`,
    );
    rowHead.firstElementChild?.setAttribute(
      'style',
      `transform: translateY(${-scrollTop}px); height: ${height}px`,
    );

    this.paintColumnHeads(columns);
    this.paintRowNumbers(rows);
    this.paintCells(rows, columns);
    this.paintSelection();
    this.placeEditor();
  }

  /** Bring a cell fully into view, the shortest distance that does it. */
  reveal(cell: Cell): void {
    const { viewport } = this.elements;
    const { columnMetrics, rowMetrics } = this.view;
    const top = rowMetrics.offset(cell.row);
    const bottom = top + rowMetrics.height(cell.row);
    const left = columnMetrics.offset(cell.column);
    const right = left + columnMetrics.width(cell.column);

    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (bottom > viewport.scrollTop + viewport.clientHeight) {
      viewport.scrollTop = bottom - viewport.clientHeight;
    }
    if (left < viewport.scrollLeft) viewport.scrollLeft = left;
    else if (right > viewport.scrollLeft + viewport.clientWidth) {
      viewport.scrollLeft = right - viewport.clientWidth;
    }
  }

  /** How many rows fit on screen — what Page Up and Page Down move by. */
  get pageRows(): number {
    return Math.max(1, Math.floor(this.elements.viewport.clientHeight / this.view.rowMetrics.defaultHeight) - 1);
  }

  /**
   * The width that would fit a column's content.
   *
   * Measured over the header and a sample of rows rather than all of them: the
   * point is a column wide enough to read, and reading two million cells to
   * decide it would take longer than the reader has. The sample starts at the top
   * of the file, which is where the widest values usually are in the columns
   * people actually widen — names, addresses, URLs.
   */
  fitColumn(column: number, max: number): number {
    const { measure } = this.elements;
    measure.className = 'measure';
    let widest = 0;
    const consider = (text: string, bold: boolean): void => {
      measure.textContent = text;
      measure.classList.toggle('measure-head', bold);
      widest = Math.max(widest, measure.offsetWidth);
    };
    const head = this.view.header(column);
    consider(head === '' ? columnLabel(column) : head, true);
    const rows = Math.min(this.view.rows, FIT_SAMPLE);
    // Measured as the cell will paint it. Wrapping, a multi-line value is as
    // wide as its widest line; not wrapping, it is one line of every line it has,
    // and fitting to the widest of them would leave the column short.
    for (let row = 0; row < rows; row += 1) {
      const value = this.view.value(row, column);
      consider(this.view.wrap ? value : oneLine(value), false);
    }
    measure.textContent = '';
    measure.classList.remove('measure-head');
    return Math.min(max, widest + FIT_PADDING);
  }

  /** The height that would fit a row's wrapped content at the current widths. */
  fitRow(row: number, max: number): number {
    const { measure } = this.elements;
    let tallest = this.view.rowMetrics.min;
    for (let column = 0; column < this.view.columns; column += 1) {
      const value = this.view.value(row, column);
      if (value === '') continue;
      measure.className = 'measure measure-wrap';
      measure.style.width = `${Math.max(20, this.view.columnMetrics.width(column) - FIT_PADDING)}px`;
      measure.textContent = value;
      tallest = Math.max(tallest, measure.offsetHeight + 6);
    }
    measure.removeAttribute('style');
    measure.className = 'measure';
    measure.textContent = '';
    return Math.min(max, tallest);
  }

  /** Open the cell editor over a cell, with a value in it. */
  beginEdit(cell: Cell, value: string, selectAll: boolean): void {
    this.editingCell = cell;
    this.editor.value = value;
    this.editor.hidden = false;
    this.placeEditor();
    this.editor.focus();
    if (selectAll) this.editor.select();
    else this.editor.setSelectionRange(value.length, value.length);
  }

  /** Close the cell editor and hand back what was typed. */
  endEdit(): string {
    const value = this.editor.value;
    this.editingCell = null;
    this.editor.hidden = true;
    this.editor.value = '';
    this.editor.removeAttribute('style');
    return value;
  }

  get isEditing(): boolean {
    return this.editingCell !== null;
  }

  dispose(): void {
    if (this.frame) cancelAnimationFrame(this.frame);
    this.elements.viewport.removeEventListener('scroll', this.onScroll);
    this.elements.table.removeEventListener('pointerdown', this.onPointerDown);
    this.elements.table.removeEventListener('dblclick', this.onDoubleClick);
    this.elements.table.removeEventListener('contextmenu', this.onContextMenu);
    window.removeEventListener('pointermove', this.onPointerMove);
    window.removeEventListener('pointerup', this.onPointerUp);
  }

  // ── Painting ───────────────────────────────────────────────────────────────

  private paintColumnHeads(window: { first: number; last: number }): void {
    const inner = this.elements.columnHead.firstElementChild as HTMLElement | null;
    if (!inner) return;
    const { columnMetrics, selection, sort } = this.view;
    const bounds = { rows: this.view.rows, columns: this.view.columns };

    for (const [column, element] of this.liveHeads) {
      if (column >= window.first && column <= window.last) continue;
      element.remove();
      this.liveHeads.delete(column);
      this.headPool.push(element);
    }

    for (let column = window.first; column <= window.last; column += 1) {
      let head = this.liveHeads.get(column);
      if (!head) {
        head = this.headPool.pop() ?? this.makeHead();
        this.liveHeads.set(column, head);
        inner.append(head);
      }
      head.style.left = `${columnMetrics.offset(column)}px`;
      head.style.width = `${columnMetrics.width(column)}px`;

      const letter = head.firstElementChild as HTMLElement;
      const name = head.children[1] as HTMLElement;
      const label = columnLabel(column);
      if (letter.textContent !== label) letter.textContent = label;
      // The palette the text editor colours with, reaching the page as a
      // custom property: the same column is the same colour in both views.
      letter.style.color = `var(--vscode-csvUltra-column${(column % 10) + 1})`;
      const title = this.view.header(column);
      name.hidden = !this.view.hasHeader;
      if (this.view.hasHeader && name.textContent !== title) name.textContent = title;

      const sorted = sort?.column === column ? (sort.direction === 'asc' ? ' asc' : ' desc') : '';
      head.className =
        'chead' +
        (hasColumn(selection, column, bounds) ? ' picked' : '') +
        (touchesColumn(selection, column) ? ' touched' : '') +
        sorted;
      head.title = title || label;
    }
  }

  private makeHead(): HTMLElement {
    const head = document.createElement('div');
    head.className = 'chead';
    const letter = document.createElement('span');
    letter.className = 'chead-letter';
    const name = document.createElement('span');
    name.className = 'chead-name';
    const handle = document.createElement('span');
    handle.className = 'chandle';
    head.append(letter, name, handle);
    return head;
  }

  private paintRowNumbers(window: { first: number; last: number }): void {
    const inner = this.elements.rowHead.firstElementChild as HTMLElement | null;
    if (!inner) return;
    const { rowMetrics, selection } = this.view;
    const bounds = { rows: this.view.rows, columns: this.view.columns };

    for (const [row, element] of this.liveNumbers) {
      if (row >= window.first && row <= window.last) continue;
      element.remove();
      this.liveNumbers.delete(row);
      this.numberPool.push(element);
    }

    for (let row = window.first; row <= window.last; row += 1) {
      let number = this.liveNumbers.get(row);
      if (!number) {
        number = this.numberPool.pop() ?? this.makeNumber();
        this.liveNumbers.set(row, number);
        inner.append(number);
      }
      number.style.top = `${rowMetrics.offset(row)}px`;
      number.style.height = `${rowMetrics.height(row)}px`;
      const label = String(this.view.rowNumber(row));
      const text = number.firstElementChild as HTMLElement;
      if (text.textContent !== label) text.textContent = label;
      number.className =
        'rhead' +
        (hasRow(selection, row, bounds) ? ' picked' : '') +
        (touchesRow(selection, row) ? ' touched' : '');
    }
  }

  private makeNumber(): HTMLElement {
    const number = document.createElement('div');
    number.className = 'rhead';
    const text = document.createElement('span');
    const handle = document.createElement('span');
    handle.className = 'rhandle';
    number.append(text, handle);
    return number;
  }

  private paintCells(
    rows: { first: number; last: number },
    columns: { first: number; last: number },
  ): void {
    const { canvas } = this.elements;
    const { columnMetrics, rowMetrics, selection } = this.view;

    for (const [row, live] of this.liveRows) {
      if (row >= rows.first && row <= rows.last) continue;
      for (const cell of live.cells) {
        cell.remove();
        this.cellPool.push(cell);
      }
      live.element.remove();
      this.liveRows.delete(row);
      this.rowPool.push(live.element);
    }

    for (let row = rows.first; row <= rows.last; row += 1) {
      let live = this.liveRows.get(row);
      if (!live) {
        const element = this.rowPool.pop() ?? document.createElement('div');
        element.className = 'row';
        live = { element, cells: [], first: -1, last: -2, generation: -1 };
        this.liveRows.set(row, live);
        canvas.append(element);
      }
      const height = rowMetrics.height(row);
      live.element.style.top = `${rowMetrics.offset(row)}px`;
      live.element.style.height = `${height}px`;
      // Centres a one-line cell vertically without a flex box — see `.cell`.
      live.element.style.lineHeight = `${height}px`;
      live.element.className = `row${this.view.zebra && row % 2 === 1 ? ' odd' : ''}`;

      const rebuild =
        live.first !== columns.first ||
        live.last !== columns.last ||
        live.generation !== this.generation;

      if (rebuild) {
        for (const cell of live.cells) {
          cell.remove();
          this.cellPool.push(cell);
        }
        live.cells = [];
        for (let column = columns.first; column <= columns.last; column += 1) {
          const cell = this.cellPool.pop() ?? document.createElement('div');
          const value = this.view.value(row, column);
          // A cell that is not wrapping is one line tall, so a value with
          // newlines in it has to become one line or all but the first of them
          // is painted below the cell's own bottom edge, where nobody sees it.
          const shown = this.view.wrap ? value : oneLine(value);
          cell.textContent = shown;
          // The rest of it is still worth being able to read.
          if (shown === value) cell.removeAttribute('title');
          else cell.title = value;
          live.cells.push(cell);
          live.element.append(cell);
        }
        live.first = columns.first;
        live.last = columns.last;
        live.generation = this.generation;
      }

      // Geometry and classes are set on every pass whether or not the cells were
      // rebuilt: a column drag changes neither the window nor the generation, and
      // a cell left at the width it was built with is a column whose header moves
      // and whose values do not.
      for (let index = 0; index < live.cells.length; index += 1) {
        const column = columns.first + index;
        const cell = live.cells[index]!;
        cell.style.left = `${columnMetrics.offset(column)}px`;
        cell.style.width = `${columnMetrics.width(column)}px`;
        const key = `${row}:${column}`;
        const active = selection.active.row === row && selection.active.column === column;
        cell.className =
          'cell' +
          (contains(selection, row, column) ? ' sel' : '') +
          (active ? ' active' : '') +
          (this.view.matchKeys.has(key) ? ' match' : '') +
          (this.view.activeMatch?.row === row && this.view.activeMatch.column === column
            ? ' match-active'
            : '') +
          (this.view.wrap ? ' wrap' : '') +
          (this.view.alignNumbers && isNumeric(cell.textContent ?? '') ? ' numeric' : '');
      }
    }
  }

  /**
   * The border round each selected block, and the ring on the active cell.
   *
   * Drawn as boxes rather than as borders on the cells: a border per cell shows
   * every internal edge, and a spreadsheet's selection is one outline round the
   * whole block with the cell you are actually on picked out inside it.
   */
  private paintSelection(): void {
    const { columnMetrics, rowMetrics, selection } = this.view;
    // A handful of boxes, rebuilt whenever the selection changes. Built as
    // elements rather than as markup: nothing in this page ever parses a string
    // into DOM, so a cell's contents can never be anything but text.
    while (this.overlay.firstChild) this.overlay.firstChild.remove();
    for (const rect of selection.ranges) {
      const top = rowMetrics.offset(rect.top);
      const left = columnMetrics.offset(rect.left);
      const box = document.createElement('div');
      box.className = 'range';
      box.style.top = `${top}px`;
      box.style.left = `${left}px`;
      box.style.width = `${columnMetrics.offset(rect.right + 1) - left}px`;
      box.style.height = `${rowMetrics.offset(rect.bottom + 1) - top}px`;
      this.overlay.append(box);
    }

    const { active } = selection;
    if (this.view.rows === 0 || this.view.columns === 0) {
      this.focusRing.hidden = true;
      return;
    }
    this.focusRing.hidden = false;
    this.focusRing.style.top = `${rowMetrics.offset(active.row)}px`;
    this.focusRing.style.left = `${columnMetrics.offset(active.column)}px`;
    this.focusRing.style.width = `${columnMetrics.width(active.column)}px`;
    this.focusRing.style.height = `${rowMetrics.height(active.row)}px`;
  }

  private placeEditor(): void {
    const cell = this.editingCell;
    if (!cell) return;
    const { columnMetrics, rowMetrics } = this.view;
    this.editor.style.top = `${rowMetrics.offset(cell.row)}px`;
    this.editor.style.left = `${columnMetrics.offset(cell.column)}px`;
    this.editor.style.minWidth = `${columnMetrics.width(cell.column)}px`;
    this.editor.style.minHeight = `${rowMetrics.height(cell.row)}px`;
  }

  // ── Pointer ────────────────────────────────────────────────────────────────

  private readonly onScroll = (): void => {
    this.schedule();
    this.callbacks.onScrolled();
  };

  private readonly onPointerDown = (event: PointerEvent): void => {
    if (event.button !== 0) return;
    const zone = this.zoneAt(event);
    if (!zone) return;

    if (zone.kind === 'column-handle') {
      const size = this.view.columnMetrics.width(zone.column);
      this.dragging = { kind: 'column', index: zone.column, start: event.clientX, size };
      this.callbacks.onColumnResized(zone.column, size, 'start');
      event.preventDefault();
    } else if (zone.kind === 'row-handle') {
      const size = this.view.rowMetrics.height(zone.row);
      this.dragging = { kind: 'row', index: zone.row, start: event.clientY, size };
      this.callbacks.onRowResized(zone.row, size, 'start');
      event.preventDefault();
    } else {
      this.dragging = { kind: 'select' };
      this.callbacks.onPick(zone.target, event);
      // A drag out of the table should still extend the selection, so the
      // listeners go on the window rather than on the element under the pointer.
      event.preventDefault();
    }

    window.addEventListener('pointermove', this.onPointerMove);
    window.addEventListener('pointerup', this.onPointerUp);
  };

  private readonly onPointerMove = (event: PointerEvent): void => {
    const drag = this.dragging;
    if (!drag) return;
    // Against the size the column had when the drag began, and the pointer's
    // distance from where it went down — never against the size it has now,
    // which is a size this drag put there and would compound.
    if (drag.kind === 'column') {
      this.callbacks.onColumnResized(drag.index, drag.size + (event.clientX - drag.start), 'move');
      return;
    }
    if (drag.kind === 'row') {
      this.callbacks.onRowResized(drag.index, drag.size + (event.clientY - drag.start), 'move');
      return;
    }
    const zone = this.zoneAt(event);
    if (zone && zone.kind === 'body') this.callbacks.onDragTo(zone.target);
  };

  private readonly onPointerUp = (event: PointerEvent): void => {
    const drag = this.dragging;
    if (drag?.kind === 'select') this.callbacks.onDragEnd();
    // A resize is closed whether or not it moved, so that the element can let go
    // of whatever it was holding for the length of the gesture.
    else if (drag?.kind === 'column') {
      this.callbacks.onColumnResized(drag.index, drag.size + (event.clientX - drag.start), 'end');
    } else if (drag?.kind === 'row') {
      this.callbacks.onRowResized(drag.index, drag.size + (event.clientY - drag.start), 'end');
    }
    this.dragging = null;
    window.removeEventListener('pointermove', this.onPointerMove);
    window.removeEventListener('pointerup', this.onPointerUp);
  };

  private readonly onDoubleClick = (event: MouseEvent): void => {
    const zone = this.zoneAt(event);
    if (!zone) return;
    // Double-clicking an edge fits that column or row to its content — the
    // spreadsheet gesture, and the one people reach for before any menu.
    if (zone.kind === 'column-handle') return this.callbacks.onFitColumn(zone.column);
    if (zone.kind === 'row-handle') return this.callbacks.onFitRow(zone.row);
    if (zone.target.kind === 'cell') {
      this.callbacks.onOpenEditor(zone.target.row, zone.target.column);
    } else if (zone.target.kind === 'column') {
      this.callbacks.onSortToggle(zone.target.column);
    }
  };

  private readonly onContextMenu = (event: MouseEvent): void => {
    const zone = this.zoneAt(event);
    if (!zone || zone.kind !== 'body') return;
    event.preventDefault();
    this.callbacks.onContext(zone.target, event);
  };

  /**
   * What is under a pointer, by arithmetic.
   *
   * The header strips and the body are separate boxes, so which one the pointer
   * is in comes from the geometry of the table, and the row and column come from
   * the same `indexAt` that placed the cells. No `event.target`, which means a
   * drag that leaves the window, crosses the gap between two cells, or ends past
   * the last row still names a cell.
   */
  private zoneAt(event: MouseEvent): Zone | null {
    const { table, viewport, columnHead, rowHead } = this.elements;
    const rect = table.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    const headHeight = columnHead.offsetHeight;
    const headWidth = rowHead.offsetWidth;
    const { columnMetrics, rowMetrics } = this.view;

    const inColumns = y < headHeight;
    const inRows = x < headWidth;
    if (inColumns && inRows) return { kind: 'body', target: { kind: 'all' } };

    if (inColumns) {
      const at = x - headWidth + viewport.scrollLeft;
      const column = columnMetrics.indexAt(at);
      const edge = columnMetrics.offset(column) + columnMetrics.width(column);
      // The zone straddles the boundary, so the handle of the column to the left
      // is reachable from the first pixels of the column to its right.
      if (Math.abs(at - edge) <= HANDLE / 2) return { kind: 'column-handle', column };
      if (at - columnMetrics.offset(column) <= HANDLE / 2 && column > 0) {
        return { kind: 'column-handle', column: column - 1 };
      }
      return { kind: 'body', target: { kind: 'column', column } };
    }

    if (inRows) {
      const at = y - headHeight + viewport.scrollTop;
      const row = rowMetrics.indexAt(at);
      const edge = rowMetrics.offset(row) + rowMetrics.height(row);
      if (Math.abs(at - edge) <= HANDLE / 2) return { kind: 'row-handle', row };
      if (at - rowMetrics.offset(row) <= HANDLE / 2 && row > 0) {
        return { kind: 'row-handle', row: row - 1 };
      }
      return { kind: 'body', target: { kind: 'row', row } };
    }

    if (this.view.rows === 0 || this.view.columns === 0) return null;
    return {
      kind: 'body',
      target: {
        kind: 'cell',
        row: rowMetrics.indexAt(y - headHeight + viewport.scrollTop),
        column: columnMetrics.indexAt(x - headWidth + viewport.scrollLeft),
      },
    };
  }
}

interface RowElements {
  element: HTMLElement;
  cells: HTMLElement[];
  first: number;
  last: number;
  generation: number;
}

type Drag =
  | { kind: 'select' }
  /** `start` is where the pointer went down; `size` what the column had then. */
  | { kind: 'column'; index: number; start: number; size: number }
  | { kind: 'row'; index: number; start: number; size: number };

type Zone =
  | { kind: 'body'; target: PickTarget }
  | { kind: 'column-handle'; column: number }
  | { kind: 'row-handle'; row: number };

function isNumeric(value: string): boolean {
  return value !== '' && parseNumber(value) !== null;
}

/**
 * A quoted value's newlines, as one line.
 *
 * A CSV field may hold as many lines as it likes, and a row is as tall as the
 * reader left it. Rather than paint the first line and hide the rest, the breaks
 * are shown where they are — an arrow the width of a character, so that what is
 * one field still reads as one field, and turning wrapping on gives the value
 * back its real shape.
 */
function oneLine(value: string): string {
  if (!value.includes('\n') && !value.includes('\r')) return value;
  return value.replace(/\r\n|[\r\n]/g, ' ↵ ');
}
