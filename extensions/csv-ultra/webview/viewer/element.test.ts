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
  readOnly: false,
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

/**
 * The keys as the browser really delivers them — dispatched at the element the
 * reader's focus is on, and left to bubble.
 *
 * The rest of the file calls `onKeydown` directly, which is the right way to ask
 * what a key *means*; it cannot answer what a key *does*, because it misses both
 * the template's binding above the table and the editor's listener below it.
 * Everything in here is about those two.
 */
describe('the keys, dispatched the way a browser dispatches them', () => {
  beforeEach(async () => {
    await load('name,price\napple,3\npear,10\n');
  });

  const press = (target: EventTarget, init: KeyboardEventInit): KeyboardEvent => {
    const event = new KeyboardEvent('keydown', { ...init, bubbles: true, cancelable: true });
    target.dispatchEvent(event);
    return event;
  };

  const editor = (): HTMLTextAreaElement => must<HTMLTextAreaElement>('.cell-editor');

  it('lets a character be typed into an editor Enter opened', async () => {
    press(must<HTMLElement>('.table'), { key: 'Enter' });
    await settle();
    expect(editor().hidden).toBe(false);
    // The table's own keydown binding sits above the editor. Cancelling a key on
    // its way up from a text box is cancelling the character.
    expect(press(editor(), { key: 'a' }).defaultPrevented).toBe(false);
  });

  it('lets a character be typed into an editor a character opened', async () => {
    press(must<HTMLElement>('.table'), { key: '3' });
    await settle();
    expect(editor().value).toBe('3');
    expect(press(editor(), { key: '4' }).defaultPrevented).toBe(false);
  });

  it('lets the find box and the row box be typed into', async () => {
    grid.handle({ type: 'command', command: 'find' });
    await settle();
    expect(press(must<HTMLInputElement>('.find-input'), { key: 'a' }).defaultPrevented).toBe(false);
    expect(press(must<HTMLInputElement>('.row-input'), { key: '7' }).defaultPrevented).toBe(false);
  });

  it('still cancels the keys the table acts on itself', async () => {
    const table = must<HTMLElement>('.table');
    expect(press(table, { key: 'ArrowDown' }).defaultPrevented).toBe(true);
    expect(press(table, { key: 'Tab' }).defaultPrevented).toBe(true);
  });

  it('closes the editor when Enter commits, rather than reopening it below', async () => {
    press(must<HTMLElement>('.table'), { key: 'F2' });
    await settle();
    editor().value = 'quince';
    press(editor(), { key: 'Enter' });
    await settle();
    expect(editor().hidden).toBe(true);
    expect(grid.selection.active).toEqual({ row: 1, column: 0 });
    expect(lastEdit()?.edit).toEqual({
      kind: 'cells',
      patches: [{ row: 1, column: 0, value: 'quince' }],
    });
  });

  it('closes the editor when Tab commits', async () => {
    press(must<HTMLElement>('.table'), { key: 'F2' });
    await settle();
    press(editor(), { key: 'Tab' });
    await settle();
    expect(editor().hidden).toBe(true);
    expect(grid.selection.active).toEqual({ row: 0, column: 1 });
  });

  it('closes the editor when Escape throws the edit away', async () => {
    press(must<HTMLElement>('.table'), { key: 'F2' });
    await settle();
    press(editor(), { key: 'Escape' });
    await settle();
    expect(editor().hidden).toBe(true);
    expect(lastEdit()).toBeUndefined();
  });
});

describe('a read-only table', () => {
  beforeEach(async () => {
    await load('name,price\napple,3\npear,10\n', { readOnly: true });
  });

  it('shows the toggle as pressed, with the padlock shut', async () => {
    const shut = must('[aria-label="Toggle read-only"]');
    expect(shut.getAttribute('aria-pressed')).toBe('true');
    const shackle = (): string => must('[aria-label="Toggle read-only"] path').getAttribute('d') ?? '';
    expect(shackle()).toBe('M5 7V4.8a3 3 0 016 0V7');

    grid.handle({ type: 'settings', settings: { ...settings, readOnly: false } });
    await settle();
    expect(must('[aria-label="Toggle read-only"]').getAttribute('aria-pressed')).toBe('false');
    // …and the padlock is open, because a tint alone does not say which way round.
    expect(shackle()).not.toBe('M5 7V4.8a3 3 0 016 0V7');
  });

  it('asks the host to flip the setting, so the next file opens the same way', () => {
    must<HTMLButtonElement>('[aria-label="Toggle read-only"]').click();
    expect(posted).toContainEqual({ type: 'run', command: 'toggleReadOnly' });
  });

  it('opens no cell editor, whichever way the reader asks for one', async () => {
    for (const key of ['Enter', 'F2', 'x']) {
      grid.onKeydown(new KeyboardEvent('keydown', { key, cancelable: true }));
    }
    await settle();
    expect(must<HTMLTextAreaElement>('.cell-editor').hidden).toBe(true);
    expect(grid.notice).toBe('This table is read-only');
    expect(lastEdit()).toBeUndefined();
  });

  it('writes nothing for Delete, a paste, or a structural change', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'Delete', cancelable: true }));
    grid.handle({ type: 'paste', text: 'x\ty' });
    for (const command of ['insertRowBelow', 'deleteRows', 'insertColumnLeft', 'deleteColumns'] as const) {
      grid.handle({ type: 'command', command });
    }
    await settle();
    expect(lastEdit()).toBeUndefined();
    expect(painted().get('0:0')).toBe('apple');
  });

  it('sorts the view but refuses to write the order down', async () => {
    grid.handle({ type: 'command', command: 'sortDescending' });
    await settle();
    expect(painted().get('0:0')).toBe('pear');
    // …and the chip offers no Write button to press.
    expect(query('[aria-label="Write the sorted order into the file"]')).toBeNull();
    grid.handle({ type: 'command', command: 'applySort' });
    await settle();
    expect(lastEdit()).toBeUndefined();
  });

  it('still copies, and offers a menu of only what it can do', async () => {
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'c', metaKey: true, cancelable: true }));
    await settle();
    expect(posted).toContainEqual({ type: 'clipboard', text: 'apple' });

    must<HTMLElement>('.table').dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 200, clientY: 200 }),
    );
    await settle();
    const labels = all('.menu-item').map((item) => item.textContent?.trim());
    expect(labels).toEqual(['Copy']);
  });

  it('closes an open editor when the setting arrives mid-edit', async () => {
    await load('name,price\napple,3\n');
    grid.onKeydown(new KeyboardEvent('keydown', { key: 'F2', cancelable: true }));
    await settle();
    expect(must<HTMLTextAreaElement>('.cell-editor').hidden).toBe(false);

    grid.handle({ type: 'settings', settings: { ...settings, readOnly: true } });
    await settle();
    expect(must<HTMLTextAreaElement>('.cell-editor').hidden).toBe(true);
    expect(lastEdit()).toBeUndefined();
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

  it('keeps a row pasted into the blank row past the end on one row', async () => {
    // Copy a row, click the blank row below the last one, paste. Every cell of it
    // belongs to the one record being appended — numbering them against a file
    // that is growing as they are written scattered them down a diagonal, a
    // record per cell.
    grid.selection = { ...grid.selection, active: { row: 3, column: 0 } };
    grid.handle({ type: 'paste', text: 'x\ty' });
    await settle();
    expect(lastEdit()?.edit).toEqual({
      kind: 'cells',
      patches: [
        { row: 3, column: 0, value: 'x' },
        { row: 3, column: 1, value: 'y' },
      ],
    });
    expect(painted().get('3:0')).toBe('x');
    expect(painted().get('3:1')).toBe('y');
  });

  it('numbers the rows of a block pasted over the bottom edge consecutively', async () => {
    grid.selection = { ...grid.selection, active: { row: 2, column: 0 } };
    grid.handle({ type: 'paste', text: 'p\tq\nr\ts\nt\tu' });
    await settle();
    expect(lastEdit()?.edit).toEqual({
      kind: 'cells',
      patches: [
        { row: 2, column: 0, value: 'p' },
        { row: 2, column: 1, value: 'q' },
        { row: 3, column: 0, value: 'r' },
        { row: 3, column: 1, value: 's' },
        { row: 4, column: 0, value: 't' },
        { row: 4, column: 1, value: 'u' },
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
