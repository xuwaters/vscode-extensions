// @vitest-environment happy-dom
import { Updates } from '@microsoft/fast-element';
import { beforeEach, describe, expect, it } from 'vitest';
import type { GridSettings, HostToWebview, WebviewToHost } from '../../src/messages.js';

const { CSV_GRID_TAG, CsvGrid } = await import('./element.js');

// fast-element 3 defines the element asynchronously, so the tests mount it the
// way the page does — parse the tag, wait for the definition, let the browser
// upgrade it — rather than constructing the class, which is exactly what a real
// engine refuses to do before the name is registered.
await customElements.whenDefined(CSV_GRID_TAG);

const settings: GridSettings = {
  headerRow: 'auto',
  rowHeight: 20,
  columnWidth: 100,
  maxColumnWidth: 400,
  // Fitting measures the DOM, and happy-dom has no layout to measure — so every
  // column would come out at the minimum and the tests would be about nothing.
  autoFitOnOpen: false,
  fontSize: 12,
  fontFamily: 'editor',
  wrap: false,
  zebraStripes: true,
  alignNumbers: true,
};

let grid: InstanceType<typeof CsvGrid>;
let posted: WebviewToHost[];

const query = <T extends Element>(selector: string): T | null =>
  grid.shadowRoot?.querySelector<T>(selector) ?? null;

const all = (selector: string): Element[] => [
  ...(grid.shadowRoot?.querySelectorAll(selector) ?? []),
];

const must = <T extends Element>(selector: string): T => {
  const found = query<T>(selector);
  if (!found) throw new Error(`the grid has no ${selector}`);
  return found;
};

/** Let the bindings, the load's deferred work and the paint frame all land. */
async function settle(): Promise<void> {
  for (let round = 0; round < 3; round += 1) {
    await Updates.next();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
  }
  await Updates.next();
}

async function load(text: string, overrides: Partial<GridSettings> = {}): Promise<void> {
  const message: HostToWebview = {
    type: 'load',
    name: 'data.csv',
    text,
    dialect: { delimiter: ',', quote: '"', newline: '\n' },
    settings: { ...settings, ...overrides },
    reason: 'open',
  };
  grid.handle(message);
  await settle();
}

/** The text of every painted cell, by `row:column`. */
function painted(): Map<string, string> {
  const cells = new Map<string, string>();
  for (const row of all('.row')) {
    const top = Number.parseFloat((row as HTMLElement).style.top);
    for (const cell of row.querySelectorAll('.cell')) {
      const left = Number.parseFloat((cell as HTMLElement).style.left);
      cells.set(`${top / 20}:${left / 100}`, cell.textContent ?? '');
    }
  }
  return cells;
}

const lastEdit = (): Extract<WebviewToHost, { type: 'edit' }> | undefined =>
  [...posted].reverse().find((message) => message.type === 'edit') as
    | Extract<WebviewToHost, { type: 'edit' }>
    | undefined;

beforeEach(async () => {
  document.body.innerHTML = `<${CSV_GRID_TAG}></${CSV_GRID_TAG}>`;
  posted = [];
  const mounted = document.body.firstElementChild;
  if (!(mounted instanceof CsvGrid)) throw new Error(`<${CSV_GRID_TAG}> was not upgraded`);
  grid = mounted;
  grid.host = { post: (message) => posted.push(message) };
  await Updates.next();
  // happy-dom has no layout, so the scroller reports no size and the sheet would
  // paint a single cell. Give it a viewport to work with.
  const viewport = must<HTMLElement>('.viewport');
  Object.defineProperty(viewport, 'clientWidth', { value: 800, configurable: true });
  Object.defineProperty(viewport, 'clientHeight', { value: 400, configurable: true });
});

describe('the states a tab passes through', () => {
  it('shows a card and no table before the host has said anything', () => {
    expect(must('.shell').classList.contains('blank')).toBe(true);
    expect(query('.card')).not.toBeNull();
  });

  it('offers the text editor for a file too large to lay out', async () => {
    grid.handle({ type: 'refused', bytes: 100 << 20, limit: 32 << 20 });
    await settle();
    expect(must('.card-body').textContent).toContain('100.0 MB');
    must<HTMLButtonElement>('.card-action').click();
    expect(posted).toContainEqual({ type: 'run', command: 'openInTextEditor' });
  });
});

describe('a loaded table', () => {
  beforeEach(async () => {
    await load('name,price\napple,3\npear,10\nplum,2\n');
  });

  it('draws the cells it can see', () => {
    const cells = painted();
    expect(cells.get('0:0')).toBe('apple');
    expect(cells.get('0:1')).toBe('3');
    expect(cells.get('2:0')).toBe('plum');
  });

  it('takes the first row as the header and titles the columns with it', () => {
    expect(grid.hasHeader).toBe(true);
    expect(all('.chead-name').map((head) => head.textContent)).toEqual(['name', 'price', '']);
    // …and the header is not one of the rows.
    expect(painted().get('0:0')).toBe('apple');
  });

  it('numbers rows by their place in the file, not in the view', () => {
    expect(all('.rhead span:first-child').map((head) => head.textContent).slice(0, 3)).toEqual([
      '2',
      '3',
      '4',
    ]);
  });

  it('carries one blank row and column past the end, to grow into', () => {
    expect(grid.rows).toBe(4);
    expect(grid.columns).toBe(3);
  });

  it('right-aligns what reads as a number', () => {
    const numeric = all('.cell.numeric').map((cell) => cell.textContent);
    expect(numeric).toContain('3');
    expect(numeric).not.toContain('apple');
  });

  it('tells the host where the reader is', () => {
    const place = posted.find((message) => message.type === 'place');
    expect(place).toMatchObject({ type: 'place', place: { rows: 4, columns: 2, row: 2 } });
  });
});

describe('moving around', () => {
  beforeEach(async () => {
    await load('name,price\napple,3\npear,10\nplum,2\n');
  });

  const key = (init: KeyboardEventInit): void => {
    grid.onKeydown(new KeyboardEvent('keydown', { ...init, bubbles: true, cancelable: true }));
  };

  it('moves the active cell with the arrows', async () => {
    key({ key: 'ArrowDown' });
    key({ key: 'ArrowRight' });
    await settle();
    expect(grid.selection.active).toEqual({ row: 1, column: 1 });
  });

  it('stops at the edges rather than wrapping', async () => {
    key({ key: 'ArrowUp' });
    key({ key: 'ArrowLeft' });
    await settle();
    expect(grid.selection.active).toEqual({ row: 0, column: 0 });
  });

  it('extends the selection with shift', async () => {
    key({ key: 'ArrowDown', shiftKey: true });
    key({ key: 'ArrowRight', shiftKey: true });
    await settle();
    expect(grid.selection.ranges).toEqual([{ top: 0, left: 0, bottom: 1, right: 1 }]);
  });

  it('selects everything with the modifier and A', async () => {
    key({ key: 'a', metaKey: true });
    await settle();
    expect(grid.selection.ranges).toEqual([{ top: 0, left: 0, bottom: 3, right: 2 }]);
  });

  it('walks a row with Tab', async () => {
    key({ key: 'Tab' });
    await settle();
    expect(grid.selection.active).toEqual({ row: 0, column: 1 });
  });
});

describe('editing a cell', () => {
  beforeEach(async () => {
    await load('name,price\napple,3\npear,10\n');
  });

  it('opens an editor when a character is typed, and starts from that character', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'x', cancelable: true }));
    await settle();
    const editor = must<HTMLTextAreaElement>('.cell-editor');
    expect(editor.hidden).toBe(false);
    expect(editor.value).toBe('x');
    expect(posted).toContainEqual({ type: 'editing', editing: true });
  });

  it('opens with the current value on F2', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'F2', cancelable: true }));
    await settle();
    expect(must<HTMLTextAreaElement>('.cell-editor').value).toBe('apple');
  });

  it('commits on Enter, in file coordinates, and moves down', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'F2', cancelable: true }));
    await settle();
    const editor = must<HTMLTextAreaElement>('.cell-editor');
    editor.value = 'quince';
    editor.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }));
    await settle();

    // Row 0 of the view is record 1 of the file, because record 0 is the header.
    expect(lastEdit()?.edit).toEqual({
      kind: 'cells',
      patches: [{ row: 1, column: 0, value: 'quince' }],
    });
    expect(grid.selection.active).toEqual({ row: 1, column: 0 });
    // …and the value is on screen before the host has answered.
    expect(painted().get('0:0')).toBe('quince');
  });

  it('throws the edit away on Escape', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'F2', cancelable: true }));
    await settle();
    const editor = must<HTMLTextAreaElement>('.cell-editor');
    editor.value = 'nope';
    editor.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', cancelable: true }));
    await settle();
    expect(lastEdit()).toBeUndefined();
    expect(painted().get('0:0')).toBe('apple');
    expect(posted).toContainEqual({ type: 'editing', editing: false });
  });

  it('writes nothing when the value did not change', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'F2', cancelable: true }));
    await settle();
    must<HTMLTextAreaElement>('.cell-editor').dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }),
    );
    await settle();
    expect(lastEdit()).toBeUndefined();
  });

  it('clears a selection with Delete', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'ArrowRight', shiftKey: true, cancelable: true }));
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'Delete', cancelable: true }));
    await settle();
    expect(lastEdit()?.edit).toEqual({
      kind: 'cells',
      patches: [
        { row: 1, column: 0, value: '' },
        { row: 1, column: 1, value: '' },
      ],
    });
  });

  it('writes into the blank row past the end, which is how a table grows', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'ArrowDown', cancelable: true }));
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'ArrowDown', cancelable: true }));
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'F2', cancelable: true }));
    await settle();
    const editor = must<HTMLTextAreaElement>('.cell-editor');
    editor.value = 'fig';
    editor.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }));
    await settle();
    // Record 3 is one past the last record of a three-record file.
    expect(lastEdit()?.edit).toEqual({
      kind: 'cells',
      patches: [{ row: 3, column: 0, value: 'fig' }],
    });
  });
});

describe('the clipboard', () => {
  beforeEach(async () => {
    await load('a,b\n1,2\n3,4\n', { headerRow: 'never' });
  });

  it('copies a selection as tab-separated text', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'ArrowDown', shiftKey: true, cancelable: true }));
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'ArrowRight', shiftKey: true, cancelable: true }));
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'c', metaKey: true, cancelable: true }));
    await settle();
    expect(posted).toContainEqual({ type: 'clipboard', text: 'a\tb\n1\t2' });
  });

  it('asks the host for the clipboard rather than reading it itself', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'v', metaKey: true, cancelable: true }));
    await settle();
    expect(posted).toContainEqual({ type: 'requestPaste' });
  });

  it('writes a pasted block from the active cell outwards', async () => {
    grid.handle({ type: 'paste', text: 'x\ty\nz\tw' });
    await settle();
    expect(lastEdit()?.edit).toEqual({
      kind: 'cells',
      patches: [
        { row: 0, column: 0, value: 'x' },
        { row: 0, column: 1, value: 'y' },
        { row: 1, column: 0, value: 'z' },
        { row: 1, column: 1, value: 'w' },
      ],
    });
  });
});

describe('sorting', () => {
  beforeEach(async () => {
    await load('name,n\ncharlie,3\nalpha,10\nbravo,2\n');
  });

  it('reorders the view without touching the file', async () => {
    grid.handle({ type: 'command', command: 'sortAscending' });
    await settle();
    const cells = painted();
    expect([cells.get('0:0'), cells.get('1:0'), cells.get('2:0')]).toEqual([
      'alpha',
      'bravo',
      'charlie',
    ]);
    expect(lastEdit()).toBeUndefined();
  });

  it('keeps the file number of each row beside it', async () => {
    grid.handle({ type: 'command', command: 'sortAscending' });
    await settle();
    expect(all('.rhead span:first-child').map((head) => head.textContent).slice(0, 3)).toEqual([
      '3',
      '4',
      '2',
    ]);
  });

  it('sorts numbers as numbers', async () => {
    grid.handle({ type: 'command', command: 'sortAscending' });
    grid.selection = { ...grid.selection, active: { row: 0, column: 1 } };
    grid.handle({ type: 'command', command: 'sortAscending' });
    await settle();
    const cells = painted();
    expect([cells.get('0:1'), cells.get('1:1'), cells.get('2:1')]).toEqual(['2', '3', '10']);
  });

  it('writes the order into the file only when asked', async () => {
    grid.handle({ type: 'command', command: 'sortDescending' });
    grid.handle({ type: 'command', command: 'applySort' });
    await settle();
    expect(lastEdit()?.edit).toEqual({
      kind: 'sort',
      column: 0,
      direction: 'desc',
      header: true,
    });
  });

  it('goes back to the file order', async () => {
    grid.handle({ type: 'command', command: 'sortAscending' });
    grid.handle({ type: 'command', command: 'clearSort' });
    await settle();
    expect(grid.sort).toBeNull();
    expect(painted().get('0:0')).toBe('charlie');
  });
});

describe('the header row', () => {
  it('can be turned off, which puts the first record back in the table', async () => {
    await load('name,price\napple,3\n');
    grid.handle({ type: 'command', command: 'toggleHeaderRow' });
    await settle();
    expect(grid.hasHeader).toBe(false);
    expect(painted().get('0:0')).toBe('name');
  });

  it('is not guessed for a file whose first row is data', async () => {
    await load('1,2\n3,4\n');
    expect(grid.hasHeader).toBe(false);
  });
});

describe('finding', () => {
  beforeEach(async () => {
    await load('name,city\nAda,London\nAlan,Paris\n');
  });

  it('opens a box and counts what it found', async () => {
    grid.handle({ type: 'command', command: 'find' });
    await settle();
    const input = must<HTMLInputElement>('.find-input');
    input.value = 'a';
    input.dispatchEvent(new Event('input'));
    await settle();
    expect(grid.findCount).toBe('1 of 3');
  });

  it('marks the matches in the table', async () => {
    grid.handle({ type: 'command', command: 'find' });
    await settle();
    const input = must<HTMLInputElement>('.find-input');
    input.value = 'paris';
    input.dispatchEvent(new Event('input'));
    await settle();
    expect(all('.cell.match').map((cell) => cell.textContent)).toEqual(['Paris']);
    expect(grid.selection.active).toEqual({ row: 1, column: 1 });
  });

  it('says so when there is nothing to find', async () => {
    grid.handle({ type: 'command', command: 'find' });
    await settle();
    const input = must<HTMLInputElement>('.find-input');
    input.value = 'zzz';
    input.dispatchEvent(new Event('input'));
    await settle();
    expect(grid.findCount).toBe('No results');
  });
});

describe('a document that changed underneath', () => {
  it('reloads without losing where the reader was', async () => {
    await load('a,b\n1,2\n3,4\n', { headerRow: 'never' });
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'ArrowDown', cancelable: true }));
    await settle();
    expect(grid.selection.active).toEqual({ row: 1, column: 0 });

    grid.handle({
      type: 'load',
      name: 'data.csv',
      text: 'a,b\nX,2\n3,4\n',
      dialect: { delimiter: ',', quote: '"', newline: '\n' },
      settings: { ...settings, headerRow: 'never' },
      reason: 'external',
    });
    await settle();
    expect(painted().get('1:0')).toBe('X');
    expect(grid.selection.active).toEqual({ row: 1, column: 0 });
  });

  it('pulls the selection back inside a table that shrank', async () => {
    await load('a\nb\nc\nd\n', { headerRow: 'never' });
    grid.handle({ type: 'command', command: 'find' });
    grid.selection = { ...grid.selection, active: { row: 3, column: 0 } };
    grid.handle({
      type: 'load',
      name: 'data.csv',
      text: 'a\n',
      dialect: { delimiter: ',', quote: '"', newline: '\n' },
      settings: { ...settings, headerRow: 'never' },
      reason: 'external',
    });
    await settle();
    expect(grid.selection.active.row).toBeLessThanOrEqual(1);
  });
});

describe('landing on a cell the reader came from', () => {
  it('takes a file row and finds it in the view', async () => {
    await load('name,n\ncharlie,3\nalpha,10\n');
    grid.handle({ type: 'select', row: 2, column: 1 });
    await settle();
    // File record 2 is view row 1 while unsorted.
    expect(grid.selection.active).toEqual({ row: 1, column: 1 });
  });
});
