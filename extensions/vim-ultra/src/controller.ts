import * as vscode from 'vscode';
import type {
  Effects,
  EngineBridge,
  EngineCommand,
  EngineMode,
  EngineSearchUi,
  EngineSelection,
  EngineSession,
} from './engine';
import { modeLabel, replaceEol, serializeSelections, typedKeys } from './util';

/** How far off-screen a search match must be before peeking centers it. */
const CENTER_PEEK_LINES = 15;

type IndentCommand = Extract<EngineCommand, { kind: 'indentLines' }>;

/** Editor selections as the engine takes them; both count UTF-16 columns. */
function enginePositions(sels: readonly vscode.Selection[]): EngineSelection[] {
  return sels.map((s) => ({
    anchor: { line: s.anchor.line, col: s.anchor.character },
    active: { line: s.active.line, col: s.active.character },
  }));
}

/**
 * Wires the vim-engine WASM sessions to VSCode:
 *  - keys in (the `type` override and keybinding commands),
 *  - effects out (edits, selections, commands, status bar, cursor style),
 *  - document/selection events mirrored back into the engine.
 *
 * The engine self-applies its own edits, so document changes made here (under
 * `applyingEdits`) are NOT mirrored back; everything else is.
 */
export class VimController implements vscode.Disposable {
  private readonly sessions = new Map<string, EngineSession>();
  private readonly status: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];
  /** All matches of the pattern being typed (vim's incsearch + hlsearch). */
  private readonly searchHighlight: vscode.TextEditorDecorationType;
  /** The match the view is peeking at — the one `<cr>` would land on. */
  private readonly searchMatch: vscode.TextEditorDecorationType;
  /** Top visible line when the search prompt opened; restored on cancel. */
  private searchViewTop: number | null = null;
  /** The editor holding search decorations, so ending clears the right one. */
  private decoratedEditor: vscode.TextEditor | null = null;
  private applyingEdits = false;
  private lastSetSelections: string | null = null;
  private enabled: boolean;
  /** The engine's last report; shown until the next key produces one. */
  private message = '';

  constructor(
    private readonly bridge: EngineBridge,
    enabled: boolean,
  ) {
    this.enabled = enabled;
    // Furthest right of the left side — after the errors/warnings item,
    // where vim-vscode puts its command line.
    this.status = vscode.window.createStatusBarItem(
      'vimUltra.primary',
      vscode.StatusBarAlignment.Left,
      Number.MIN_SAFE_INTEGER,
    );
    this.status.name = 'Vim Ultra';
    this.searchHighlight = vscode.window.createTextEditorDecorationType({
      backgroundColor: new vscode.ThemeColor('editor.findMatchHighlightBackground'),
      overviewRulerColor: new vscode.ThemeColor('editorOverviewRuler.findMatchForeground'),
      border: '1px solid',
      borderColor: new vscode.ThemeColor('editor.findMatchHighlightBorder'),
    });
    this.searchMatch = vscode.window.createTextEditorDecorationType({
      backgroundColor: new vscode.ThemeColor('editor.findMatchBackground'),
      overviewRulerColor: new vscode.ThemeColor('editorOverviewRuler.findMatchForeground'),
      border: '2px solid',
      borderColor: new vscode.ThemeColor('editor.findMatchBorder'),
    });
    this.disposables.push(
      vscode.workspace.onDidChangeTextDocument((e) => this.onDocChange(e)),
      vscode.window.onDidChangeTextEditorSelection((e) => this.onSelectionChange(e)),
      vscode.window.onDidChangeActiveTextEditor((e) => this.onActiveEditor(e)),
      vscode.workspace.onDidCloseTextDocument((doc) => this.dropSession(doc)),
    );
    void this.syncContext();
    this.onActiveEditor(vscode.window.activeTextEditor);
  }

  get isEnabled(): boolean {
    return this.enabled;
  }

  async setEnabled(enabled: boolean): Promise<void> {
    this.enabled = enabled;
    await this.syncContext();
    const editor = vscode.window.activeTextEditor;
    if (!enabled) {
      this.status.hide();
      if (editor) {
        this.applySearchUi(editor, { kind: 'cancelled' }, true);
        this.setCursorStyle(editor, 'insert');
      }
      return;
    }
    this.onActiveEditor(editor);
  }

  /** The `type` command override: printable keys while an editor has focus. */
  async type(text: string): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    const session = this.usableSession(editor);
    if (!editor || !session || session.mode() === 'insert') {
      return vscode.commands.executeCommand('default:type', { text });
    }
    for (const key of typedKeys(text)) {
      await this.sendKey(editor, session, key);
    }
  }

  /** Keybinding-driven keys: `<esc>`, `<cr>`, `<bs>`, `<c-r>`. */
  async key(key: string): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    const session = this.usableSession(editor);
    if (!editor || !session) return;
    await this.sendKey(editor, session, key);
  }

  /** Ctrl-d/u/f/b: scroll by half/full pages, keeping visual selections. */
  async scroll(dir: 'up' | 'down', by: 'half' | 'page'): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    const session = this.usableSession(editor);
    if (!editor || !session) return;
    const visible = editor.visibleRanges[0];
    const height = visible ? visible.end.line - visible.start.line : 20;
    const value = Math.max(1, by === 'half' ? Math.floor(height / 2) : height);
    await vscode.commands.executeCommand('cursorMove', {
      to: dir,
      by: 'wrappedLine',
      value,
      select: session.mode().startsWith('visual'),
    });
    // The selection-change event syncs the engine's cursor.
  }

  dispose(): void {
    for (const d of this.disposables) d.dispose();
    for (const s of this.sessions.values()) s.dispose();
    this.sessions.clear();
    this.status.dispose();
    this.searchHighlight.dispose();
    this.searchMatch.dispose();
  }

  // ---- key -> effects ------------------------------------------------------

  private async sendKey(
    editor: vscode.TextEditor,
    session: EngineSession,
    key: string,
  ): Promise<void> {
    const fx = session.key(key);
    if (!fx) return;
    // Only keys refresh the message, so an incidental cursor sync (or the
    // selection change our own edit causes) does not wipe it.
    this.message = fx.message ?? '';
    await this.applyEffects(editor, session, fx);
  }

  private async applyEffects(
    editor: vscode.TextEditor,
    session: EngineSession,
    fx: Effects,
    external = false,
  ): Promise<void> {
    if (fx.edits.length > 0) {
      const eol = editor.document.eol === vscode.EndOfLine.CRLF ? '\r\n' : '\n';
      this.applyingEdits = true;
      let ok = false;
      try {
        ok = await editor.edit((builder) => {
          for (const e of fx.edits) {
            builder.replace(
              new vscode.Range(e.start.line, e.start.col, e.end.line, e.end.col),
              replaceEol(e.text, eol),
            );
          }
        });
      } catch {
        // Overlapping ranges are rejected outright — two cursors reaching
        // for the same text is the way to produce them. Treated like any
        // other refusal: nothing landed, so rebuild the mirror.
        ok = false;
      } finally {
        this.applyingEdits = false;
      }
      if (!ok) {
        // The document refused the edit (readonly, conflict): the mirror has
        // already advanced past reality, so rebuild it.
        this.resync(editor, session);
        return;
      }
    }
    // While a search prompt is open the cursor stays put and the *viewport*
    // follows the matches; leave the selection alone so the two don't fight.
    if (fx.selections.length > 0 && fx.search?.kind !== 'active') {
      const sels = fx.selections.map(
        (s) => new vscode.Selection(s.anchor.line, s.anchor.col, s.active.line, s.active.col),
      );
      if (serializeSelections(editor.selections) !== serializeSelections(sels)) {
        this.lastSetSelections = serializeSelections(sels);
        editor.selections = sels;
      }
      // A cancelled search restores its own viewport below instead.
      if (fx.search?.kind !== 'cancelled') {
        editor.revealRange(
          new vscode.Range(sels[0].active, sels[0].active),
          vscode.TextEditorRevealType.Default,
        );
      }
    }
    // An indent operator emits one command per cursor; VSCode indents all of
    // a multi-selection at once, so they run as a single, undoable step.
    const indents = fx.commands.filter((c) => c.kind === 'indentLines');
    if (indents.length > 0) await this.indentLines(editor, session, indents);
    for (const cmd of fx.commands) {
      if (cmd.kind !== 'indentLines') await this.runCommand(editor, cmd);
    }
    this.applySearchUi(editor, fx.search, external);
    this.updateUi(editor, fx.mode, fx.pending, editor.selections.length);
  }

  /**
   * The incremental-search protocol: while the prompt is `active`, highlight
   * the matches and scroll the peeked one into view without moving the
   * cursor; when it closes, clear the paint and either keep the viewport
   * (`committed`) or scroll back to where the search began (`cancelled`).
   * A cancel caused by the user clicking elsewhere (`external`) keeps their
   * new viewport instead of yanking it back.
   */
  private applySearchUi(
    editor: vscode.TextEditor,
    ui: EngineSearchUi | undefined,
    external: boolean,
  ): void {
    if (!ui) return;
    if (ui.kind === 'active') {
      this.searchViewTop ??= editor.visibleRanges[0]?.start.line ?? 0;
      this.decoratedEditor = editor;
      const ranges: vscode.Range[] = [];
      const m = ui.matches;
      for (let i = 0; i + 2 < m.length; i += 3) {
        ranges.push(new vscode.Range(m[i], m[i + 1], m[i], m[i + 2]));
      }
      editor.setDecorations(this.searchHighlight, ranges);
      if (ui.current) {
        const [line, start, end] = ui.current;
        const current = new vscode.Range(line, start, line, end);
        editor.setDecorations(this.searchMatch, [current]);
        this.revealPeek(editor, current);
      } else {
        // Nothing to peek at (yet): drift back to where the search began.
        editor.setDecorations(this.searchMatch, []);
        this.revealTop(editor, this.searchViewTop);
      }
      return;
    }
    const decorated = this.decoratedEditor ?? editor;
    decorated.setDecorations(this.searchHighlight, []);
    decorated.setDecorations(this.searchMatch, []);
    this.decoratedEditor = null;
    if (ui.kind === 'cancelled' && !external && this.searchViewTop !== null) {
      this.revealTop(editor, this.searchViewTop);
    }
    this.searchViewTop = null;
  }

  /** Scroll a peeked match into view: nearby scrolls minimally, a far jump
   *  centers (vim-vscode's heuristic). */
  private revealPeek(editor: vscode.TextEditor, range: vscode.Range): void {
    const visible = editor.visibleRanges[0];
    const far =
      !visible ||
      visible.start.line - range.start.line >= CENTER_PEEK_LINES ||
      range.start.line - visible.end.line >= CENTER_PEEK_LINES;
    editor.revealRange(
      range,
      far ? vscode.TextEditorRevealType.InCenter : vscode.TextEditorRevealType.Default,
    );
  }

  private revealTop(editor: vscode.TextEditor, line: number): void {
    editor.revealRange(
      new vscode.Range(line, 0, line, 0),
      vscode.TextEditorRevealType.AtTop,
    );
  }

  private async runCommand(editor: vscode.TextEditor, cmd: EngineCommand): Promise<void> {
    switch (cmd.kind) {
      case 'undo':
      case 'redo':
        await vscode.commands.executeCommand(cmd.kind);
        break;
      case 'scroll':
        await vscode.commands.executeCommand('revealLine', {
          lineNumber: editor.selection.active.line,
          at: cmd.to,
        });
        break;
      // `o`/`O`. VSCode's own line-insert is the one that knows the
      // language's indent rules — it is what `<cr>` in insert mode runs — so
      // the new line lands indented instead of at column 0. It opens a line
      // at every cursor, which is why the engine emits it once. The edit and
      // the cursor move it makes mirror back like any other outside change.
      case 'openLine':
        await vscode.commands.executeCommand(
          cmd.above ? 'editor.action.insertLineBefore' : 'editor.action.insertLineAfter',
        );
        break;
      // `gh`: the hover for the symbol under the cursor.
      case 'showHover':
        await vscode.commands.executeCommand('editor.action.showHover');
        break;
    }
  }

  /**
   * `>`/`<`: hand the affected line spans to VSCode's indenter as one
   * multi-selection, then land each cursor on the first non-blank of its
   * first line, like vim. One key never mixes indent with dedent, so the
   * first command's direction is the direction.
   */
  private async indentLines(
    editor: vscode.TextEditor,
    session: EngineSession,
    cmds: readonly IndentCommand[],
  ): Promise<void> {
    const doc = editor.document;
    const spans = cmds.map((c) => {
      const endLine = Math.min(c.endLine, doc.lineCount - 1);
      return new vscode.Selection(c.startLine, 0, endLine, doc.lineAt(endLine).text.length);
    });
    this.lastSetSelections = serializeSelections(spans);
    editor.selections = spans;
    await vscode.commands.executeCommand(
      cmds[0].dedent ? 'editor.action.outdentLines' : 'editor.action.indentLines',
    );
    const cursors = cmds.map((c) => {
      const col = doc.lineAt(c.startLine).firstNonWhitespaceCharacterIndex;
      return new vscode.Selection(c.startLine, col, c.startLine, col);
    });
    this.lastSetSelections = serializeSelections(cursors);
    editor.selections = cursors;
    session.setCursors(enginePositions(cursors));
  }

  // ---- event mirroring -----------------------------------------------------

  private onDocChange(e: vscode.TextDocumentChangeEvent): void {
    if (this.applyingEdits) return; // engine self-applied these already
    const session = this.sessions.get(e.document.uri.toString());
    if (!session || e.contentChanges.length === 0) return;
    session.applyChanges(
      e.contentChanges.map((c) => ({
        startLine: c.range.start.line,
        startCol: c.range.start.character,
        endLine: c.range.end.line,
        endCol: c.range.end.character,
        text: c.text,
      })),
    );
  }

  private onSelectionChange(e: vscode.TextEditorSelectionChangeEvent): void {
    if (!this.enabled || e.textEditor !== vscode.window.activeTextEditor) return;
    const session = this.usableSession(e.textEditor);
    if (!session) return;
    const ser = serializeSelections(e.selections);
    if (ser === this.lastSetSelections) return; // our own write, engine knows
    this.lastSetSelections = null;
    // Every selection, not just the primary: extra cursors (cmd+alt+arrow,
    // cmd+d) are cursors the engine drives too.
    const fx = session.setCursors(enginePositions(e.selections));
    if (fx) void this.applyEffects(e.textEditor, session, fx, true);
  }

  private onActiveEditor(editor: vscode.TextEditor | undefined): void {
    if (!this.enabled) return;
    const session = this.usableSession(editor);
    if (!editor || !session) {
      this.status.hide();
      return;
    }
    // Re-clamp the cursors for normal mode and refresh the UI.
    const fx = session.setCursors(enginePositions(editor.selections));
    if (fx) void this.applyEffects(editor, session, fx, true);
  }

  private dropSession(doc: vscode.TextDocument): void {
    const key = doc.uri.toString();
    this.sessions.get(key)?.dispose();
    this.sessions.delete(key);
  }

  // ---- sessions & UI ---------------------------------------------------------

  private usableSession(editor: vscode.TextEditor | undefined): EngineSession | null {
    if (!this.enabled || !editor) return null;
    const scheme = editor.document.uri.scheme;
    if (scheme === 'output' || scheme === 'debug') return null;
    const key = editor.document.uri.toString();
    let session = this.sessions.get(key);
    if (!session) {
      const created = this.bridge.createSession(
        editor.document.getText(),
        editor.selection.active.line,
        editor.selection.active.character,
      );
      if (!created) return null;
      this.sessions.set(key, created);
      session = created;
    }
    return session;
  }

  private resync(editor: vscode.TextEditor, session: EngineSession): void {
    this.message = ''; // the report described edits that never landed
    this.applySearchUi(editor, { kind: 'cancelled' }, true);
    session.reset(
      editor.document.getText(),
      editor.selection.active.line,
      editor.selection.active.character,
    );
    // The rebuild is single-cursor; hand the extra ones back.
    if (editor.selections.length > 1) {
      session.setCursors(enginePositions(editor.selections));
    }
    this.updateUi(editor, session.mode(), '', editor.selections.length);
  }

  private updateUi(
    editor: vscode.TextEditor,
    mode: EngineMode,
    pending: string,
    cursors: number,
  ): void {
    this.status.text = modeLabel(mode, pending, this.message, cursors);
    this.status.show();
    this.setCursorStyle(editor, mode);
    void vscode.commands.executeCommand('setContext', 'vimUltra.mode', mode);
  }

  private setCursorStyle(editor: vscode.TextEditor, mode: EngineMode): void {
    editor.options = {
      ...editor.options,
      cursorStyle:
        mode === 'insert'
          ? vscode.TextEditorCursorStyle.Line
          : vscode.TextEditorCursorStyle.Block,
    };
  }

  private async syncContext(): Promise<void> {
    await vscode.commands.executeCommand('setContext', 'vimUltra.active', this.enabled);
  }
}
