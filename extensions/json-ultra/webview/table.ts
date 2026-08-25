// The virtualized, read-only table. A few hundred recycled DOM nodes
// repositioned each animation frame cover any row count the host is
// willing to send; nothing below the fold exists. Headers and row
// numbers sit *outside* the scroller (a sticky header inside a two-axis
// scroller is a fight with the browser nobody wins) and are translated
// by the scroll offsets each frame.
//
// Cell text is always assigned via `textContent` — never markup.

import type { HostToWebview, WebviewToHost } from '../src/messages.js';
import type { JsonlTable } from '../src/types.js';

const ROW_HEIGHT = 24;
const MIN_COLUMN = 64;
const MAX_COLUMN = 420;
const CHAR_WIDTH = 7.5;
const MEASURE_SAMPLE = 200;
const OVERDRAW_ROWS = 4;

export interface Host {
  post(message: WebviewToHost): void;
}

interface LiveRow {
  element: HTMLElement;
  cells: HTMLElement[];
  first: number;
  last: number;
  generation: number;
}

export class TableView {
  private table: JsonlTable = { columns: [], rows: [], total: 0, truncated: false };
  private name = '';
  private widths: number[] = [];
  private offsets: number[] = [];
  private generation = 0;
  private frame = 0;
  private selected: { row: number; column: number } | null = null;

  private readonly root: HTMLElement;
  private toolbarName!: HTMLElement;
  private toolbarMeta!: HTMLElement;
  private tableEl!: HTMLElement;
  private colheadInner!: HTMLElement;
  private rowheadInner!: HTMLElement;
  private viewport!: HTMLElement;
  private canvas!: HTMLElement;

  private readonly liveRows = new Map<number, LiveRow>();
  private readonly rowPool: HTMLElement[] = [];
  private readonly cellPool: HTMLElement[] = [];
  private readonly liveHeads = new Map<number, HTMLElement>();
  private readonly liveNums = new Map<number, HTMLElement>();

  constructor(
    root: HTMLElement,
    private readonly host: Host,
  ) {
    this.root = root;
    this.buildChrome();
  }

  handle(message: HostToWebview): void {
    switch (message.type) {
      case 'load':
        this.name = message.name;
        this.table = message.table;
        this.selected = null;
        this.measureColumns();
        this.invalidate();
        this.updateToolbar();
        break;
      case 'refused':
        this.showCard(
          'File too large',
          `This file is ${megabytes(message.bytes)} — the preview refuses files over ` +
            `${megabytes(message.limit)}. Raise jsonUltra.preview.maxFileSizeBytes to override.`,
        );
        break;
      case 'noParser':
        this.showCard(
          'Parser unavailable',
          'The WASM analyzer bundle is missing. Run `pnpm run build:wasm` in extensions/json-ultra.',
        );
        break;
      case 'visible':
        this.schedule();
        break;
    }
  }

  private buildChrome(): void {
    this.root.replaceChildren();
    const stack = el('div', 'stack');

    const toolbar = el('div', 'toolbar');
    this.toolbarName = el('span', 'name');
    this.toolbarMeta = el('span', 'meta');
    const spacer = el('span', 'spacer');
    const openText = el('button', '') as HTMLButtonElement;
    openText.textContent = 'Open Text Editor';
    openText.addEventListener('click', () => this.host.post({ type: 'openText' }));
    toolbar.append(this.toolbarName, this.toolbarMeta, spacer, openText);

    this.tableEl = el('div', 'table');
    const corner = el('div', 'corner');
    const colhead = el('div', 'colhead');
    this.colheadInner = el('div', 'colhead-inner');
    colhead.append(this.colheadInner);
    const rowhead = el('div', 'rowhead');
    this.rowheadInner = el('div', 'rowhead-inner');
    rowhead.append(this.rowheadInner);
    this.viewport = el('div', 'viewport');
    this.canvas = el('div', 'canvas');
    this.viewport.append(this.canvas);
    this.tableEl.append(corner, colhead, rowhead, this.viewport);

    this.viewport.addEventListener('scroll', () => this.schedule(), { passive: true });
    new ResizeObserver(() => this.schedule()).observe(this.viewport);
    this.canvas.addEventListener('click', (event) => this.onCellClick(event, false));
    this.canvas.addEventListener('dblclick', (event) => this.onCellClick(event, true));

    stack.append(toolbar, this.tableEl);
    this.root.replaceChildren(stack);
  }

  private showCard(title: string, message: string): void {
    const card = el('div', 'card');
    const heading = el('div', 'title');
    heading.textContent = title;
    const body = el('div', '');
    body.textContent = message;
    card.append(heading, body);
    this.tableEl.replaceChildren(card);
  }

  private updateToolbar(): void {
    this.toolbarName.textContent = this.name;
    const rows = this.table.total.toLocaleString();
    const cols = this.table.columns.length.toLocaleString();
    const truncated = this.table.truncated
      ? ` (showing first ${this.table.rows.length.toLocaleString()})`
      : '';
    this.toolbarMeta.textContent = `${rows} rows${truncated} · ${cols} columns`;
  }

  /** Estimate column widths from the header and a sample of rows. */
  private measureColumns(): void {
    const { columns, rows } = this.table;
    const chars = columns.map((c) => c.length);
    const sample = Math.min(rows.length, MEASURE_SAMPLE);
    for (let r = 0; r < sample; r += 1) {
      const cells = rows[r].cells;
      for (let c = 0; c < cells.length && c < chars.length; c += 1) {
        // The first line of a multi-line cell is what the row shows.
        const newline = cells[c].indexOf('\n');
        const length = newline === -1 ? cells[c].length : newline;
        if (length > chars[c]) chars[c] = length;
      }
    }
    this.widths = chars.map((n) =>
      Math.max(MIN_COLUMN, Math.min(MAX_COLUMN, Math.round(n * CHAR_WIDTH) + 17)),
    );
    this.offsets = new Array(this.widths.length + 1);
    this.offsets[0] = 0;
    for (let i = 0; i < this.widths.length; i += 1) {
      this.offsets[i + 1] = this.offsets[i] + this.widths[i];
    }
    const digits = String(Math.max(1, this.table.total)).length;
    this.tableEl.style.setProperty(
      '--rowhead-width',
      `${Math.max(48, 20 + digits * 8)}px`,
    );
  }

  private invalidate(): void {
    this.generation += 1;
    this.liveHeads.forEach((head) => head.remove());
    this.liveHeads.clear();
    this.liveNums.forEach((num) => num.remove());
    this.liveNums.clear();
    // Rebuild the grid chrome in case a card replaced it.
    if (!this.tableEl.querySelector('.viewport')) this.buildChrome();
    this.schedule();
  }

  private schedule(): void {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.render();
    });
  }

  private columnWindow(left: number, right: number): { first: number; last: number } {
    const count = this.widths.length;
    if (count === 0) return { first: 0, last: -1 };
    let first = 0;
    while (first < count - 1 && this.offsets[first + 1] <= left) first += 1;
    let last = first;
    while (last < count - 1 && this.offsets[last + 1] < right) last += 1;
    return { first, last };
  }

  private render(): void {
    const rows = this.table.rows;
    const width = this.offsets[this.offsets.length - 1] ?? 0;
    const height = rows.length * ROW_HEIGHT;
    this.canvas.style.width = `${width}px`;
    this.canvas.style.height = `${height}px`;

    const { scrollTop, scrollLeft, clientWidth, clientHeight } = this.viewport;
    const firstRow = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERDRAW_ROWS);
    const lastRow = Math.min(
      rows.length - 1,
      Math.ceil((scrollTop + clientHeight) / ROW_HEIGHT) + OVERDRAW_ROWS,
    );
    const cols = this.columnWindow(scrollLeft, scrollLeft + clientWidth);

    this.colheadInner.style.transform = `translateX(${-scrollLeft}px)`;
    this.colheadInner.style.width = `${width}px`;
    this.rowheadInner.style.transform = `translateY(${-scrollTop}px)`;
    this.rowheadInner.style.height = `${height}px`;

    this.paintHeads(cols);
    this.paintRowNumbers(firstRow, lastRow);
    this.paintRows(firstRow, lastRow, cols);
  }

  private paintHeads(cols: { first: number; last: number }): void {
    for (const [index, head] of this.liveHeads) {
      if (index >= cols.first && index <= cols.last) continue;
      head.remove();
      this.liveHeads.delete(index);
      this.cellPool.push(head);
    }
    for (let c = cols.first; c <= cols.last; c += 1) {
      let head = this.liveHeads.get(c);
      if (!head) {
        head = this.cellPool.pop() ?? document.createElement('div');
        head.className = 'cell';
        this.liveHeads.set(c, head);
        this.colheadInner.append(head);
      }
      head.style.left = `${this.offsets[c]}px`;
      head.style.width = `${this.widths[c]}px`;
      const name = this.table.columns[c] ?? '';
      head.textContent = name;
      head.title = name;
    }
  }

  private paintRowNumbers(first: number, last: number): void {
    for (const [index, num] of this.liveNums) {
      if (index >= first && index <= last) continue;
      num.remove();
      this.liveNums.delete(index);
    }
    for (let r = first; r <= last; r += 1) {
      let num = this.liveNums.get(r);
      if (!num) {
        num = document.createElement('div');
        num.className = 'rownum';
        const row = this.table.rows[r];
        num.textContent = String(row.line + 1);
        num.title = 'Reveal this line in the text editor';
        num.addEventListener('click', () => this.host.post({ type: 'openLine', line: row.line }));
        this.liveNums.set(r, num);
        this.rowheadInner.append(num);
      }
      num.style.top = `${r * ROW_HEIGHT}px`;
    }
  }

  private paintRows(
    first: number,
    last: number,
    cols: { first: number; last: number },
  ): void {
    for (const [index, live] of this.liveRows) {
      if (index >= first && index <= last) continue;
      for (const cell of live.cells) {
        cell.remove();
        this.cellPool.push(cell);
      }
      live.element.remove();
      this.liveRows.delete(index);
      this.rowPool.push(live.element);
    }
    for (let r = first; r <= last; r += 1) {
      let live = this.liveRows.get(r);
      if (!live) {
        const element = this.rowPool.pop() ?? document.createElement('div');
        live = { element, cells: [], first: -1, last: -2, generation: -1 };
        this.liveRows.set(r, live);
        this.canvas.append(element);
      }
      live.element.className = `row${r % 2 === 1 ? ' odd' : ''}`;
      live.element.style.top = `${r * ROW_HEIGHT}px`;
      const stale =
        live.first !== cols.first || live.last !== cols.last || live.generation !== this.generation;
      if (stale) {
        for (const cell of live.cells) {
          cell.remove();
          this.cellPool.push(cell);
        }
        live.cells = [];
        const cells = this.table.rows[r].cells;
        for (let c = cols.first; c <= cols.last; c += 1) {
          const cell = this.cellPool.pop() ?? document.createElement('div');
          cell.className = 'cell';
          cell.style.left = `${this.offsets[c]}px`;
          cell.style.width = `${this.widths[c]}px`;
          cell.textContent = cells[c] ?? '';
          cell.dataset.row = String(r);
          cell.dataset.column = String(c);
          live.element.append(cell);
          live.cells.push(cell);
        }
        live.first = cols.first;
        live.last = cols.last;
        live.generation = this.generation;
      }
      for (const cell of live.cells) {
        const isSelected =
          this.selected !== null &&
          Number(cell.dataset.row) === this.selected.row &&
          Number(cell.dataset.column) === this.selected.column;
        cell.classList.toggle('selected', isSelected);
      }
    }
  }

  private onCellClick(event: MouseEvent, isDouble: boolean): void {
    const target = event.target;
    if (!(target instanceof HTMLElement) || !target.classList.contains('cell')) return;
    const row = Number(target.dataset.row);
    const column = Number(target.dataset.column);
    if (!Number.isInteger(row) || !Number.isInteger(column)) return;
    this.selected = { row, column };
    this.schedule();
    if (isDouble) {
      const text = this.table.rows[row]?.cells[column] ?? '';
      this.host.post({ type: 'copy', text });
    }
  }
}

function el(tag: string, className: string): HTMLElement {
  const element = document.createElement(tag);
  if (className) element.className = className;
  return element;
}

function megabytes(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
