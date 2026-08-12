import * as vscode from 'vscode';
import type {
  Effects,
  EngineBridge,
  EngineCommand,
  EngineMode,
  EngineSession,
} from './engine';
import { modeLabel, replaceEol, serializeSelections, typedKeys } from './util';

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
  private applyingEdits = false;
  private lastSetSelections: string | null = null;
  private enabled: boolean;

  constructor(
    private readonly bridge: EngineBridge,
    enabled: boolean,
  ) {
    this.enabled = enabled;
    this.status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    this.status.name = 'Vim Ultra';
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
      if (editor) this.setCursorStyle(editor, 'insert');
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
  }

  // ---- key -> effects ------------------------------------------------------

  private async sendKey(
    editor: vscode.TextEditor,
    session: EngineSession,
    key: string,
  ): Promise<void> {
    const fx = session.key(key);
    if (fx) await this.applyEffects(editor, session, fx);
  }

  private async applyEffects(
    editor: vscode.TextEditor,
    session: EngineSession,
    fx: Effects,
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
    if (fx.selections.length > 0) {
      const sels = fx.selections.map(
        (s) => new vscode.Selection(s.anchor.line, s.anchor.col, s.active.line, s.active.col),
      );
      if (serializeSelections(editor.selections) !== serializeSelections(sels)) {
        this.lastSetSelections = serializeSelections(sels);
        editor.selections = sels;
      }
      editor.revealRange(
        new vscode.Range(sels[0].active, sels[0].active),
        vscode.TextEditorRevealType.Default,
      );
    }
    for (const cmd of fx.commands) {
      await this.runCommand(editor, session, cmd);
    }
    this.updateUi(editor, fx.mode, fx.pending);
  }

  private async runCommand(
    editor: vscode.TextEditor,
    session: EngineSession,
    cmd: EngineCommand,
  ): Promise<void> {
    switch (cmd.kind) {
      case 'undo':
      case 'redo':
        await vscode.commands.executeCommand(cmd.kind);
        break;
      case 'indentLines': {
        const doc = editor.document;
        const endLine = Math.min(cmd.endLine, doc.lineCount - 1);
        const span = new vscode.Selection(
          cmd.startLine,
          0,
          endLine,
          doc.lineAt(endLine).text.length,
        );
        this.lastSetSelections = serializeSelections([span]);
        editor.selections = [span];
        await vscode.commands.executeCommand(
          cmd.dedent ? 'editor.action.outdentLines' : 'editor.action.indentLines',
        );
        // Land like vim: first non-blank of the first affected line.
        const col = doc.lineAt(cmd.startLine).firstNonWhitespaceCharacterIndex;
        const cursor = new vscode.Selection(cmd.startLine, col, cmd.startLine, col);
        this.lastSetSelections = serializeSelections([cursor]);
        editor.selections = [cursor];
        session.setPosition(cmd.startLine, col);
        break;
      }
      case 'scroll':
        await vscode.commands.executeCommand('revealLine', {
          lineNumber: editor.selection.active.line,
          at: cmd.to,
        });
        break;
    }
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
    if (e.selections.length !== 1) return; // native multi-cursor: stand back
    const session = this.usableSession(e.textEditor);
    if (!session) return;
    const ser = serializeSelections(e.selections);
    if (ser === this.lastSetSelections) return; // our own write, engine knows
    this.lastSetSelections = null;
    const sel = e.selections[0];
    const fx = sel.isEmpty
      ? session.setPosition(sel.active.line, sel.active.character)
      : session.setSelection(
          { line: sel.anchor.line, col: sel.anchor.character },
          { line: sel.active.line, col: sel.active.character },
        );
    if (fx) void this.applyEffects(e.textEditor, session, fx);
  }

  private onActiveEditor(editor: vscode.TextEditor | undefined): void {
    if (!this.enabled) return;
    const session = this.usableSession(editor);
    if (!editor || !session) {
      this.status.hide();
      return;
    }
    // Re-clamp the cursor for normal mode and refresh the UI.
    const fx = session.setPosition(editor.selection.active.line, editor.selection.active.character);
    if (fx) void this.applyEffects(editor, session, fx);
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
    session.reset(
      editor.document.getText(),
      editor.selection.active.line,
      editor.selection.active.character,
    );
    this.updateUi(editor, session.mode(), '');
  }

  private updateUi(editor: vscode.TextEditor, mode: EngineMode, pending: string): void {
    this.status.text = modeLabel(mode, pending);
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
