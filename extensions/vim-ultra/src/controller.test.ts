import * as fs from 'fs';
import { createRequire } from 'module';
import * as path from 'path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { VimController } from './controller';
import { EngineSession, type SessionFactory } from './engine';
import {
  EndOfLine,
  Range,
  Selection,
  TextEditorCursorStyle,
  TextEditorSelectionChangeKind as Kind,
  resetStub,
  stubCommands,
  stubEvents,
  window,
  type StubDocument,
  type StubEditor,
} from './vscodeStub';

/**
 * The controller against a stubbed editor: how VSCode's events land is what
 * decides whether the engine's cursors still stand where the user's do.
 * `wasm/` is a build artifact, so these are skipped until `pnpm run
 * build:wasm` has produced it.
 */
const entry = path.join(__dirname, '..', 'wasm', 'vim_engine.js');
const built = fs.existsSync(entry);

interface WasmModule {
  Session: new (text: string, line: number, col: number) => never;
}

function sessionFactory(): SessionFactory {
  const mod = createRequire(entry)(entry) as WasmModule;
  return {
    createSession: (text, line, col) =>
      new EngineSession(new mod.Session(text, line, col)),
  };
}

/** A stub editor that remembers what was painted on it, newest per type. */
interface DecoratedEditor extends StubEditor {
  decorations: Map<unknown, Range[]>;
}

function makeEditor(uri: string, text: string, line: number, col: number): DecoratedEditor {
  const lines = text.split('\n');
  const document: StubDocument = {
    uri: { scheme: 'file', toString: () => uri },
    eol: EndOfLine.LF,
    getText: () => text,
    lineCount: lines.length,
    lineAt: (n: number) => ({
      text: lines[n],
      firstNonWhitespaceCharacterIndex: lines[n].search(/\S|$/),
    }),
  };
  const decorations = new Map<unknown, Range[]>();
  return {
    document,
    selections: [new Selection(line, col, line, col)],
    get selection() {
      return this.selections[0];
    },
    options: {},
    visibleRanges: [new Range(0, 0, lines.length - 1, 0)],
    decorations,
    edit: () => Promise.resolve(true),
    revealRange: () => {},
    setDecorations: (type: unknown, ranges: Range[]) => void decorations.set(type, ranges),
  };
}

/** Every range painted on the editor, whatever decoration type it came in. */
function painted(editor: DecoratedEditor): string[] {
  return [...editor.decorations.values()]
    .flat()
    .map((r) => `${r.start.line}:${r.start.character}-${r.end.line}:${r.end.character}`)
    .sort();
}

/** Make `editor` the active one, the way VSCode announces the switch. */
function activate(editor: StubEditor): void {
  window.activeTextEditor = editor;
  stubEvents.activeEditor.fire(editor);
}

/** The editor's cursor moved for a reason of VSCode's own. */
function moveCursor(editor: StubEditor, line: number, col: number): void {
  editor.selections = [new Selection(line, col, line, col)];
  stubEvents.selection.fire({ textEditor: editor, selections: editor.selections });
}

/**
 * VSCode made a selection with a body. `kind` is how it says who made it:
 * the pointer, the keyboard, a command — or nothing at all, which is what
 * several of its own commands report.
 */
function select(editor: StubEditor, sel: Selection, kind?: number): void {
  editor.selections = [sel];
  stubEvents.selection.fire({ textEditor: editor, selections: editor.selections, kind });
}

/** The mode the controller last published to the `when`-clause context. */
function contextMode(): unknown {
  const set = stubCommands.filter(
    (c) => c.command === 'setContext' && c.args[0] === 'vimUltra.mode',
  );
  return set.at(-1)?.args[1];
}

describe.skipIf(!built)('vim controller', () => {
  let controller: VimController;

  beforeEach(() => {
    resetStub();
  });

  afterEach(() => {
    controller?.dispose();
  });

  it('moves down from where a restored cursor sits, not from the top', async () => {
    // Going to a definition in another file and coming back: VSCode hands the
    // editor over before restoring the saved position, then moves the cursor
    // onto it. The engine has to hear that second move, or the next `j` runs
    // from line 0 — the bug this pins.
    const typ = makeEditor('file:///paper.typ', '#import "x"\n#bibliography("01.bib")\ntail', 1, 15);
    const bib = makeEditor('file:///01.bib', '@article{a}\n', 0, 0);
    window.activeTextEditor = typ;
    controller = new VimController(sessionFactory(), true);

    // A motion in the typst file, so the engine has written this selection.
    await controller.type('l');
    await controller.type('h');
    expect(typ.selections[0].active).toEqual({ line: 1, character: 15 });

    activate(bib); // F12 lands at the top of the bibliography
    typ.selections = [new Selection(0, 0, 0, 0)]; // the editor comes back bare
    activate(typ); // ctrl+- returns to the typst file
    moveCursor(typ, 1, 15); // ...and VSCode restores the position

    await controller.type('j');
    expect(typ.selections[0].active.line).toBe(2);
  });

  it('mirrors a cursor move it never saw before running the next key', async () => {
    // The same desync from the other direction: an event the controller drops
    // (it arrives for an editor that is not the active one yet) must not leave
    // the engine one position behind.
    const doc = makeEditor('file:///paper.typ', 'alpha\nbeta\ngamma', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    doc.selections = [new Selection(2, 0, 2, 0)]; // moved with no event at all
    await controller.type('k');
    expect(doc.selections[0].active.line).toBe(1);
  });

  it('stays in insert when shift+arrow selects', async () => {
    const doc = makeEditor('file:///a.ts', 'hello world', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    await controller.type('i');
    expect(contextMode()).toBe('insert');

    select(doc, new Selection(0, 0, 0, 5), Kind.Keyboard); // shift+right ×5
    expect(contextMode()).toBe('insert');
    expect(doc.options.cursorStyle).toBe(TextEditorCursorStyle.Line);
    // The selection is the user's; the controller must not collapse it.
    expect(doc.selections).toHaveLength(1);
    expect(doc.selections[0].anchor).toEqual({ line: 0, character: 0 });
    expect(doc.selections[0].active).toEqual({ line: 0, character: 5 });
  });

  it('stays in insert when a suggestion lands on a placeholder', async () => {
    // Tab on the suggest widget: the typed word is replaced and VSCode
    // selects the snippet placeholder it landed on. That selection is not a
    // reason to leave insert.
    const doc = makeEditor('file:///a.ts', 'con', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    await controller.type('A'); // append: insert at the end of "con"
    expect(contextMode()).toBe('insert');

    stubEvents.docChange.fire({
      document: doc.document,
      contentChanges: [{ range: new Range(0, 0, 0, 3), text: 'concat(sep)' }],
    });
    select(doc, new Selection(0, 7, 0, 10), Kind.Command); // "sep" selected

    expect(contextMode()).toBe('insert');
    expect(doc.selections[0].active).toEqual({ line: 0, character: 10 });
  });

  it('stays in normal mode when the find widget leaves a match selected', async () => {
    // cmd+f, enter, escape: the match stays highlighted. Visual mode there
    // turns the next `j` into a drag — the bug this pins.
    const doc = makeEditor('file:///a.ts', 'hello world\nsecond line', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    select(doc, new Selection(0, 6, 0, 11), Kind.Command); // "world" found
    expect(contextMode()).toBe('normal');

    await controller.type('j');
    expect(contextMode()).toBe('normal');
    expect(doc.selections).toHaveLength(1);
    expect(doc.selections[0].anchor).toEqual(doc.selections[0].active);
    expect(doc.selections[0].active.line).toBe(1);
  });

  it('paints easymotion labels over the visible lines and clears them on the jump', async () => {
    const doc = makeEditor('file:///a.ts', 'foo bar baz', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    // The default trigger is the leader twice, and the leader is the space.
    await controller.type(' ');
    await controller.type(' ');
    await controller.type('w');
    // Two word starts ahead of the cursor: one label each, the rest dimmed.
    expect(painted(doc)).toContain('0:4-0:5');
    expect(painted(doc)).toContain('0:8-0:9');
    expect(doc.selections[0].active).toEqual({ line: 0, character: 0 });

    await controller.type('s'); // the second marker key
    expect(doc.selections[0].active).toEqual({ line: 0, character: 8 });
    expect(painted(doc)).toEqual([]);
  });

  it('leaves a lone space as a motion', async () => {
    const doc = makeEditor('file:///a.ts', 'foo bar', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    await controller.type(' ');
    expect(doc.selections[0].active).toEqual({ line: 0, character: 0 });
    await controller.type('l');
    expect(doc.selections[0].active).toEqual({ line: 0, character: 2 });
    expect(painted(doc)).toEqual([]);
  });

  it('still enters visual when the pointer drags a selection', async () => {
    const doc = makeEditor('file:///a.ts', 'hello world', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    select(doc, new Selection(0, 0, 0, 5), Kind.Mouse);
    expect(contextMode()).toBe('visual');
  });
});
