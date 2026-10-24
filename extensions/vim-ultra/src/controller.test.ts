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
  TabInputText,
  TextEditorCursorStyle,
  TextEditorSelectionChangeKind as Kind,
  resetStub,
  stubClipboard,
  stubCommandEffects,
  stubCommands,
  stubDocuments,
  stubEvents,
  stubQuickPicks,
  stubStatus,
  window,
  workspace,
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

/**
 * A stub editor that remembers what was painted on it, newest per type, and
 * the text of every edit it was asked to make, in order.
 */
interface DecoratedEditor extends StubEditor {
  decorations: Map<unknown, Range[]>;
  edits: string[];
}

function makeEditor(uri: string, text: string, line: number, col: number): DecoratedEditor {
  const lines = text.split('\n');
  const document: StubDocument = {
    uri: { scheme: 'file', path: uri.replace(/^\w+:\/\//, ''), toString: () => uri },
    eol: EndOfLine.LF,
    isDirty: false,
    isUntitled: false,
    getText: () => text,
    lineCount: lines.length,
    lineAt: (n: number) => ({
      text: lines[n],
      firstNonWhitespaceCharacterIndex: lines[n].search(/\S|$/),
    }),
  };
  const decorations = new Map<unknown, Range[]>();
  const edits: string[] = [];
  return {
    document,
    selections: [new Selection(line, col, line, col)],
    get selection() {
      return this.selections[0];
    },
    options: {},
    visibleRanges: [new Range(0, 0, lines.length - 1, 0)],
    decorations,
    edits,
    edit: (cb: (b: unknown) => void) => {
      cb({ replace: (_range: Range, text: string) => edits.push(text) });
      return Promise.resolve(true);
    },
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

  it('paints the labels of a char search and jumps to the one picked', async () => {
    const doc = makeEditor('file:///a.ts', 'a.b.c.d', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    await controller.type(' ');
    await controller.type(' ');
    await controller.type('s'); // both ways at once
    expect(painted(doc)).toEqual([]); // nothing yet: the char is still to come
    await controller.type('.');
    expect(painted(doc)).toContain('0:1-0:2');

    await controller.type('s'); // the second marker key
    expect(doc.selections[0].active).toEqual({ line: 0, character: 3 });
    expect(painted(doc)).toEqual([]);
  });

  it('paints the labels of a slash search once <cr> closes it', async () => {
    const doc = makeEditor('file:///a.ts', 'foo\nfoo\nfoo', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    await controller.type(' ');
    await controller.type(' ');
    await controller.type('/');
    await controller.type('f');
    await controller.type('o');
    expect(painted(doc)).toEqual([]); // still typing
    await controller.key('<cr>');
    expect(painted(doc)).toContain('1:0-1:1');

    await controller.type('s');
    expect(doc.selections[0].active).toEqual({ line: 2, character: 0 });
  });

  it('narrows a two-key label as the keys are typed', async () => {
    // 30 targets against 27 marker keys: the last four need two keystrokes,
    // which is every real search on a screenful of code.
    const doc = makeEditor('file:///a.ts', 'x.'.repeat(30), 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    await controller.type(' ');
    await controller.type(' ');
    await controller.type('f');
    await controller.type('.');
    expect(painted(doc).length).toBeGreaterThan(27);

    await controller.type(';'); // the group prefix
    expect(painted(doc).length).toBeGreaterThan(0);
    await controller.type('a'); // ...and the key inside it
    expect(doc.selections[0].active).toEqual({ line: 0, character: 53 });
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

  describe('file commands', () => {
    /** Type `:{line}` and run it, the way the keys arrive. */
    async function ex(line: string): Promise<void> {
      await controller.type(`:${line}`);
      await controller.key('<cr>');
    }

    function didRun(command: string): boolean {
      return stubCommands.some((c) => c.command === command);
    }

    /** Saving clears the dirty flag of whatever the active editor shows. */
    function saveClears(): void {
      stubCommandEffects.set('workbench.action.files.save', () => {
        if (window.activeTextEditor) window.activeTextEditor.document.isDirty = false;
      });
    }

    /** Closing takes the editor away, and VSCode says so. */
    function closeRemoves(): void {
      for (const command of [
        'workbench.action.closeActiveEditor',
        'workbench.action.revertAndCloseActiveEditor',
      ]) {
        stubCommandEffects.set(command, () => {
          window.activeTextEditor = undefined;
          stubEvents.activeEditor.fire(undefined);
        });
      }
    }

    /** Nothing touched the editor's UI after the close: no stale session use. */
    function nothingAfter(command: string): void {
      const at = stubCommands.findIndex((c) => c.command === command);
      expect(at).toBeGreaterThanOrEqual(0);
      expect(stubCommands.slice(at + 1).map((c) => c.command)).not.toContain('setContext');
    }

    it(':w saves through VSCode and reports it the way Vim does', async () => {
      const doc = makeEditor('file:///src/a.ts', 'one\ntwo', 0, 0);
      doc.document.isDirty = true;
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);
      saveClears();

      await ex('w');
      expect(didRun('workbench.action.files.save')).toBe(true);
      expect(stubStatus.text).toBe('"src/a.ts" 2L, 7B written');
      expect(didRun('workbench.action.closeActiveEditor')).toBe(false);
    });

    it(':w with a file name says it is not supported and writes nothing', async () => {
      const doc = makeEditor('file:///a.ts', 'one', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await ex('w other.ts');
      expect(didRun('workbench.action.files.save')).toBe(false);
      expect(stubStatus.text).toBe('file names are not supported: other.ts');
    });

    it(':q refuses a modified document, and :q! throws the changes away', async () => {
      const doc = makeEditor('file:///a.ts', 'one', 0, 0);
      doc.document.isDirty = true;
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);
      closeRemoves();

      await ex('q');
      expect(stubStatus.text).toBe('E37: No write since last change (add ! to override)');
      expect(didRun('workbench.action.closeActiveEditor')).toBe(false);

      await ex('q!');
      expect(didRun('workbench.action.revertAndCloseActiveEditor')).toBe(true);
      nothingAfter('workbench.action.revertAndCloseActiveEditor');
    });

    it(':q closes a modified document shown in another editor without losing it', async () => {
      const doc = makeEditor('file:///a.ts', 'one', 0, 0);
      doc.document.isDirty = true;
      const tab = { input: new TabInputText(doc.document.uri) };
      window.tabGroups.all = [{ tabs: [tab] }, { tabs: [tab] }];
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);
      closeRemoves();

      await ex('q!');
      expect(didRun('workbench.action.revertAndCloseActiveEditor')).toBe(false);
      expect(didRun('workbench.action.closeActiveEditor')).toBe(true);
    });

    it('ZZ and :x write only when modified, then close; :wq always writes', async () => {
      const clean = makeEditor('file:///a.ts', 'one\ntwo', 0, 0);
      window.activeTextEditor = clean;
      controller = new VimController(sessionFactory(), true);
      saveClears();
      closeRemoves();

      await controller.type('ZZ');
      expect(didRun('workbench.action.files.save')).toBe(false);
      expect(didRun('workbench.action.closeActiveEditor')).toBe(true);
      nothingAfter('workbench.action.closeActiveEditor');

      const dirty = makeEditor('file:///b.ts', 'x', 0, 0);
      dirty.document.isDirty = true;
      activate(dirty);
      stubCommands.length = 0;
      await ex('x');
      expect(stubCommands.map((c) => c.command)).toEqual(
        expect.arrayContaining(['workbench.action.files.save', 'workbench.action.closeActiveEditor']),
      );
      expect(dirty.document.isDirty).toBe(false);

      const again = makeEditor('file:///c.ts', 'y', 0, 0);
      activate(again);
      stubCommands.length = 0;
      await ex('wq');
      expect(didRun('workbench.action.files.save')).toBe(true);
      expect(didRun('workbench.action.closeActiveEditor')).toBe(true);
    });

    it('drops the keys that follow ZZ in the same burst', async () => {
      const doc = makeEditor('file:///a.ts', 'one\ntwo', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);
      closeRemoves();

      await controller.type('ZZj');
      expect(doc.selections[0].active).toEqual({ line: 0, character: 0 });
    });

    it('ZQ closes without writing', async () => {
      const doc = makeEditor('file:///a.ts', 'one', 0, 0);
      doc.document.isDirty = true;
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);
      closeRemoves();

      await controller.type('ZQ');
      expect(didRun('workbench.action.files.save')).toBe(false);
      expect(didRun('workbench.action.revertAndCloseActiveEditor')).toBe(true);
    });

    it(':qa refuses while any document is modified and shows it; :qa! discards', async () => {
      const doc = makeEditor('file:///a.ts', 'one', 0, 0);
      const other = makeEditor('file:///b.ts', 'two', 0, 0);
      other.document.isDirty = true;
      workspace.textDocuments = [doc.document, other.document];
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await ex('qa');
      expect(stubCommands.find((c) => c.command === 'showTextDocument')?.args[0]).toBe(
        other.document,
      );
      expect(didRun('workbench.action.closeAllEditors')).toBe(false);
      activate(other); // VSCode switches to the editor it was asked to show
      expect(stubStatus.text).toBe('E162: No write since last change for buffer "b.ts"');

      activate(doc);
      stubCommands.length = 0;
      await ex('qa!');
      expect(stubCommands.map((c) => c.command).filter((c) => !c.startsWith('setContext'))).toEqual([
        'showTextDocument',
        'workbench.action.revertAndCloseActiveEditor',
        'workbench.action.closeAllEditors',
      ]);
    });

    it(':wa saves everything and closes nothing; :wqa saves and closes', async () => {
      const doc = makeEditor('file:///a.ts', 'one', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await ex('wa');
      expect(didRun('workbench.action.files.saveAll')).toBe(true);
      expect(didRun('workbench.action.closeAllEditors')).toBe(false);
      await ex('wqa');
      expect(didRun('workbench.action.closeAllEditors')).toBe(true);
    });

    it(':e reloads from disk, but not over unsaved changes without !', async () => {
      const doc = makeEditor('file:///a.ts', 'one', 0, 0);
      doc.document.isDirty = true;
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await ex('e');
      expect(stubStatus.text).toBe('E37: No write since last change (add ! to override)');
      expect(didRun('workbench.action.files.revert')).toBe(false);
      await ex('e!');
      expect(didRun('workbench.action.files.revert')).toBe(true);
    });

    it(':>> shifts twice', async () => {
      const doc = makeEditor('file:///a.ts', 'one\ntwo', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await ex('%>>');
      const shifts = stubCommands.filter((c) => c.command === 'editor.action.indentLines');
      expect(shifts).toHaveLength(2);
      expect(doc.selections[0].active).toEqual({ line: 0, character: 0 });
    });
  });

  it('scrolls by lines with ctrl+e and ctrl+y, keeping a selection', async () => {
    const doc = makeEditor('file:///a.ts', 'a\nb\nc\nd', 1, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    await controller.type('3');
    await controller.key('<c-e>');
    expect(ran('editorScroll')).toEqual([
      { to: 'down', by: 'line', value: 3, revealCursor: true },
    ]);

    // VSCode reveals the cursor by collapsing the selection: not in visual.
    stubCommands.length = 0;
    await controller.type('v');
    await controller.key('<c-y>');
    expect(ran('editorScroll')).toEqual([
      { to: 'up', by: 'line', value: 1, revealCursor: false },
    ]);
    expect(contextMode()).toBe('visual');
  });

  it('goes to the definition on gd', async () => {
    const doc = makeEditor('file:///a.ts', 'let x = foo();', 0, 8);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    await controller.type('g');
    await controller.type('d');
    expect(stubCommands.map((c) => c.command)).toContain('editor.action.revealDefinition');
    expect(doc.selections[0].active).toEqual({ line: 0, character: 8 });
  });

  describe('registers and marks', () => {
    it('puts a yank into "+ on the system clipboard', async () => {
      const doc = makeEditor('file:///a.ts', 'one\ntwo', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await controller.type('"+yy');
      await controller.type('"*yiw');
      await controller.type('yy'); // the unnamed register stays off the clipboard
      expect(stubClipboard.writes).toEqual(['one\n', 'one']);
    });

    it('reads the system clipboard before a key that puts from it', async () => {
      const doc = makeEditor('file:///a.ts', 'one\ntwo', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);
      stubClipboard.text = 'pasted\r\n'; // linewise, and Windows line breaks

      await controller.type('"+p');
      expect(doc.edits).toEqual(['pasted\n']);
      await controller.type(':pu *');
      await controller.key('<cr>');
      expect(doc.edits.at(-1)).toBe('pasted\n');
    });

    it('holds the keys typed during a clipboard read back until it lands', async () => {
      const doc = makeEditor('file:///a.ts', 'one\ntwo', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);
      let settle: (text: string) => void = () => {};
      stubClipboard.read = () => new Promise((resolve) => (settle = resolve));

      await controller.type('"+');
      const put = controller.type('P'); // waits on the clipboard...
      const del = controller.type('x'); // ...and this waits on it
      await Promise.resolve();
      expect(doc.edits).toEqual([]);
      settle('x\n');
      await Promise.all([put, del]);
      // The put went in first, and `x` took its character.
      expect(doc.edits).toEqual(['x\n', '']);
    });

    it('lists the registers in a QuickPick', async () => {
      const doc = makeEditor('file:///a.ts', 'one\ntwo', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await controller.type('"ayy');
      await controller.type(':reg a');
      await controller.key('<cr>');
      expect(stubQuickPicks).toHaveLength(1);
      expect(stubQuickPicks[0].options).toMatchObject({ title: ':registers' });
      expect(stubQuickPicks[0].items).toEqual([{ label: '"a', description: 'line', detail: 'one^J' }]);
    });

    it("opens the document a file mark is in and lands on it", async () => {
      const a = makeEditor('file:///src/a.ts', 'one\n  two', 1, 3);
      const b = makeEditor('file:///src/b.ts', 'x', 0, 0);
      stubDocuments.set('file:///src/a.ts', a.document);
      window.activeTextEditor = a;
      controller = new VimController(sessionFactory(), true);

      await controller.type('mA');
      activate(b);
      stubCommands.length = 0;
      await controller.type("'A");
      const shown = stubCommands.find((c) => c.command === 'showTextDocument');
      expect(shown?.args[0]).toBe(a.document);
      // `'A` is linewise: the first non-blank, which only the host can see.
      expect((shown?.args[1] as { selection: Range }).selection.start).toEqual({ line: 1, character: 2 });

      // `:marks` names the other document the way the explorer does.
      await controller.type(':marks A');
      await controller.key('<cr>');
      expect(stubQuickPicks.at(-1)?.items).toEqual([
        { label: 'A', description: 'line 2, col 3', detail: 'src/a.ts' },
      ]);
    });

    it('walks the jumplist with ctrl+o and ctrl+i', async () => {
      const doc = makeEditor('file:///a.ts', 'a\nb\nc\nd', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await controller.type('G');
      await controller.key('<c-o>');
      expect(doc.selections[0].active.line).toBe(0);
      await controller.key('<c-i>');
      expect(doc.selections[0].active.line).toBe(3);
    });
  });

  describe('dot-repeat and macros', () => {
    /** Insert-mode typing as VSCode delivers it: the change, then the move. */
    function typeInsert(editor: StubEditor, text: string): void {
      const at = editor.selections[0].active;
      stubEvents.docChange.fire({
        document: editor.document,
        contentChanges: [{ range: new Range(at.line, at.character, at.line, at.character), text }],
      });
      moveCursor(editor, at.line, at.character + text.length);
    }

    it('records with q, saying so, and runs the register with @', async () => {
      const doc = makeEditor('file:///a.ts', 'foo bar\nbaz qux\nend', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await controller.type('qa');
      expect(stubStatus.text).toBe('-- NORMAL -- recording @a');
      await controller.type('ciw');
      expect(stubStatus.text).toBe('-- INSERT -- recording @a');
      typeInsert(doc, 'X');
      await controller.key('<esc>');
      await controller.type('jq');
      expect(stubStatus.text).toBe('-- NORMAL --');
      await controller.type(':reg a');
      await controller.key('<cr>');
      expect(stubQuickPicks.at(-1)?.items).toEqual([
        { label: '"a', description: 'char', detail: 'ciwX<esc>j' },
      ]);

      doc.edits.length = 0;
      stubCommands.length = 0;
      await controller.type('@a');
      // The word goes, and the text typed in its place goes in as an edit of
      // the engine's — not through the editor's typing, which would
      // auto-indent and auto-close it all over again.
      expect(doc.edits).toEqual(['', 'X']);
      expect(ran('default:type')).toEqual([]);
      expect(doc.selections[0].active).toEqual({ line: 2, character: 0 });
      expect(contextMode()).toBe('normal');
    });

    it('stops the register at a key that fails', async () => {
      const doc = makeEditor('file:///a.ts', 'abc', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      await controller.type('qbfZxq'); // the `f` finds nothing even now
      doc.edits.length = 0;
      await controller.type('@b');
      expect(doc.edits).toEqual([]);
    });

    it('repeats o through the line-insert, then puts the text in', async () => {
      const doc = makeEditor('file:///a.ts', 'a\nb', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);
      // The line-insert opens a line under the cursor; its change and move
      // come back as events, the way VSCode's do.
      stubCommandEffects.set('editor.action.insertLineAfter', () => {
        const line = doc.selections[0].active.line;
        const end = 1; // every line here is one character long
        stubEvents.docChange.fire({
          document: doc.document,
          contentChanges: [{ range: new Range(line, end, line, end), text: '\n' }],
        });
        moveCursor(doc, line + 1, 0);
      });

      await controller.type('o');
      typeInsert(doc, 'foo');
      await controller.key('<esc>');
      await controller.type('j');
      doc.edits.length = 0;
      await controller.type('.');
      expect(ran('editor.action.insertLineAfter')).toHaveLength(2);
      expect(doc.edits).toEqual(['foo']);
      expect(contextMode()).toBe('normal');
      expect(doc.selections[0].active).toEqual({ line: 3, character: 2 });
    });

    it('stops a register that runs itself too deep', async () => {
      const doc = makeEditor('file:///a.ts', 'abc', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);

      // `@z` while recording runs the empty `z`, which does nothing: `z`
      // ends up running itself before it gets to its `l`, forever.
      await controller.type('qz@zlq');
      await controller.type('@z');
      expect(stubStatus.text).toBe('E169: Command too recursive');
      expect(doc.selections[0].active).toEqual({ line: 0, character: 1 });
    });

    it('stops a register on <esc> and holds typed keys back until it ends', async () => {
      const doc = makeEditor('file:///a.ts', 'one\ntwo\nthree', 0, 0);
      window.activeTextEditor = doc;
      controller = new VimController(sessionFactory(), true);
      await controller.type('qy"+Pjq');
      expect(doc.selections[0].active.line).toBe(1);

      let reading = false;
      let settle: (text: string) => void = () => {};
      stubClipboard.read = () => {
        reading = true;
        return new Promise((resolve) => (settle = resolve));
      };
      doc.edits.length = 0;
      const run = controller.type('@y');
      for (let i = 0; i < 100 && !reading; i++) await new Promise((r) => setTimeout(r, 0));
      expect(reading).toBe(true);
      const esc = controller.key('<esc>'); // stops the run...
      const k = controller.type('k'); // ...and this waits for it
      settle('x');
      await Promise.all([run, esc, k]);
      // The put waiting on the clipboard landed; the `j` after it never ran,
      // and the `k` typed meanwhile ran once the register had stopped.
      expect(doc.edits).toEqual(['x']);
      expect(doc.selections[0].active.line).toBe(0);
    });
  });

  it('lands on the last digit after ctrl+a', async () => {
    const doc = makeEditor('file:///a.ts', 'x = 41;', 0, 0);
    window.activeTextEditor = doc;
    controller = new VimController(sessionFactory(), true);

    await controller.key('<c-a>');
    expect(doc.selections[0].active).toEqual({ line: 0, character: 5 });
  });
});

/** The arguments of every run of `command`, oldest first. */
function ran(command: string): unknown[] {
  return stubCommands.filter((c) => c.command === command).map((c) => c.args[0]);
}

describe('keybindings', () => {
  interface Binding {
    key: string;
    command: string;
    args?: unknown;
    when?: string;
  }
  const manifest = JSON.parse(
    fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8'),
  ) as { contributes: { keybindings: Binding[] } };
  const keys = manifest.contributes.keybindings.filter((b) => b.command === 'vimUltra.key');

  it('leaves the ctrl keys to the editor while typing', () => {
    // `ctrl+a`/`ctrl+e` are line start/end in a macOS text field, `ctrl+y`
    // and `ctrl+x` are VSCode's own: insert mode must not lose them.
    const ctrl = keys.filter((b) => b.key.startsWith('ctrl+'));
    expect(ctrl.map((b) => b.key)).toEqual(
      expect.arrayContaining(['ctrl+a', 'ctrl+x', 'ctrl+e', 'ctrl+y', 'ctrl+o', 'ctrl+i']),
    );
    for (const b of ctrl) {
      expect(b.when, b.key).toMatch(/vimUltra\.mode (!= 'insert'|== 'normal')/);
      expect(b.args).toBe(`<c-${b.key.slice('ctrl+'.length)}>`);
    }
  });
});
