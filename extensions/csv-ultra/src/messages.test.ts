import { describe, expect, it } from 'vitest';
import {
  GRID_COMMANDS,
  HOST_COMMANDS,
  MAX_CELL,
  MAX_INDEX,
  parseLayout,
  parseWebviewMessage,
  type GridLayout,
} from './messages.js';

const layout: GridLayout = {
  widths: [[0, 120]],
  heights: [[3, 60]],
  sort: { column: 1, direction: 'desc' },
  header: true,
  wrap: false,
  fontSize: 14,
  scrollTop: 40,
  scrollLeft: 0,
  activeRow: 2,
  activeColumn: 1,
};

describe('messages the host will accept', () => {
  it('takes the ones with nothing in them', () => {
    expect(parseWebviewMessage({ type: 'ready' })).toEqual({ type: 'ready' });
    expect(parseWebviewMessage({ type: 'requestPaste' })).toEqual({ type: 'requestPaste' });
  });

  it('refuses anything that is not a message at all', () => {
    expect(parseWebviewMessage(null)).toBeNull();
    expect(parseWebviewMessage('ready')).toBeNull();
    expect(parseWebviewMessage({})).toBeNull();
    expect(parseWebviewMessage({ type: 'somethingElse' })).toBeNull();
  });
});

describe('an edit, which is what makes these guards matter', () => {
  const edit = (value: unknown): unknown => parseWebviewMessage({ type: 'edit', edit: value });

  it('takes a well-formed cell write', () => {
    expect(edit({ kind: 'cells', patches: [{ row: 2, column: 3, value: 'x' }] })).toEqual({
      type: 'edit',
      edit: { kind: 'cells', patches: [{ row: 2, column: 3, value: 'x' }] },
    });
  });

  it('refuses a row that is not a whole number in range', () => {
    for (const row of [-1, 1.5, Number.NaN, MAX_INDEX + 1, '4', null]) {
      expect(edit({ kind: 'cells', patches: [{ row, column: 0, value: 'x' }] })).toBeNull();
    }
  });

  it('refuses a value that is not a bounded string', () => {
    expect(edit({ kind: 'cells', patches: [{ row: 0, column: 0, value: 7 }] })).toBeNull();
    expect(
      edit({ kind: 'cells', patches: [{ row: 0, column: 0, value: 'x'.repeat(MAX_CELL + 1) }] }),
    ).toBeNull();
  });

  it('refuses a patch list that is not a list', () => {
    expect(edit({ kind: 'cells', patches: 'all of them' })).toBeNull();
  });

  it('takes inserted rows, and refuses an empty insertion', () => {
    expect(edit({ kind: 'insertRows', at: 0, rows: [['a', 'b']] })).toEqual({
      type: 'edit',
      edit: { kind: 'insertRows', at: 0, rows: [['a', 'b']] },
    });
    expect(edit({ kind: 'insertRows', at: 0, rows: [] })).toBeNull();
    expect(edit({ kind: 'insertRows', at: 0, rows: [['a', 7]] })).toBeNull();
  });

  it('takes deletions of real indices only', () => {
    expect(edit({ kind: 'deleteRows', rows: [3, 1] })).toEqual({
      type: 'edit',
      edit: { kind: 'deleteRows', rows: [3, 1] },
    });
    expect(edit({ kind: 'deleteRows', rows: [] })).toBeNull();
    expect(edit({ kind: 'deleteRows', rows: [-2] })).toBeNull();
  });

  it('bounds how many columns one insertion may add', () => {
    expect(edit({ kind: 'insertColumns', at: 1, count: 2 })).not.toBeNull();
    expect(edit({ kind: 'insertColumns', at: 1, count: 0 })).toBeNull();
    expect(edit({ kind: 'insertColumns', at: 1, count: 999_999 })).toBeNull();
  });

  it('takes a sort, and refuses one with no direction', () => {
    expect(edit({ kind: 'sort', column: 2, direction: 'asc', header: true })).not.toBeNull();
    expect(edit({ kind: 'sort', column: 2, direction: 'sideways', header: true })).toBeNull();
    expect(edit({ kind: 'sort', column: 2, direction: 'asc' })).toBeNull();
  });

  it('refuses an edit of a kind it has never heard of', () => {
    expect(edit({ kind: 'dropTable' })).toBeNull();
  });
});

describe('the commands the page may ask for', () => {
  it('takes only the ones the chrome has buttons for', () => {
    for (const command of HOST_COMMANDS) {
      expect(parseWebviewMessage({ type: 'run', command })).toEqual({ type: 'run', command });
    }
    // The point of the union: `executeCommand` is on the other side of this.
    expect(parseWebviewMessage({ type: 'run', command: 'workbench.action.quit' })).toBeNull();
    expect(parseWebviewMessage({ type: 'run', command: 42 })).toBeNull();
  });

  it('lists every grid command exactly once', () => {
    expect(new Set(GRID_COMMANDS).size).toBe(GRID_COMMANDS.length);
  });
});

describe('the clipboard, which goes to the system', () => {
  it('takes text', () => {
    expect(parseWebviewMessage({ type: 'clipboard', text: 'a\tb' })).toEqual({
      type: 'clipboard',
      text: 'a\tb',
    });
  });

  it('refuses a gigabyte of it', () => {
    expect(parseWebviewMessage({ type: 'clipboard', text: 'x'.repeat(64_000_001) })).toBeNull();
  });
});

describe('a layout', () => {
  it('round-trips one it wrote', () => {
    expect(parseLayout(layout)).toEqual(layout);
  });

  it('refuses one that would place the reader nowhere', () => {
    expect(parseLayout({ ...layout, activeRow: -1 })).toBeNull();
    expect(parseLayout({ ...layout, scrollTop: 'top' })).toBeNull();
    expect(parseLayout({ ...layout, widths: [[0, 'wide']] })).toBeNull();
    expect(parseLayout(null)).toBeNull();
  });

  it('reads leniently where a missing answer is still usable', () => {
    // A layout parked by an older version predates whatever it is missing.
    const older = { ...layout, sort: undefined, header: undefined, wrap: undefined, fontSize: 0 };
    expect(parseLayout(older)).toMatchObject({ sort: null, header: null, wrap: null, fontSize: null });
  });

  it('drops a sort that names no direction rather than refusing the layout', () => {
    expect(parseLayout({ ...layout, sort: { column: 1 } })?.sort).toBeNull();
  });
});

describe('an error report', () => {
  it('is truncated rather than trusted', () => {
    const message = parseWebviewMessage({
      type: 'error',
      message: 'x'.repeat(5000),
      context: 'y'.repeat(500),
    });
    expect(message).toMatchObject({ type: 'error' });
    expect((message as { message: string }).message).toHaveLength(2000);
    expect((message as { context: string }).context).toHaveLength(200);
  });
});
