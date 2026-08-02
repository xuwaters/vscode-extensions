import * as vscode from 'vscode';
import { applyTaskToggle, resolveLink } from './actions';
import type { EngineSession } from './engine';
import { isWebviewToHost, type WebviewToHost } from './messages';
import { NO_HISTORY, PreviewRenderer } from './renderer';
import { isMarkdownPath } from './util';

const DEBOUNCE_MS = 150;

/** VSCode's built-in text editor, for handing a tab back to the source. */
const TEXT_EDITOR = 'default';

/**
 * VSCode's own Reopen With, which swaps the editor *inside* the active tab.
 * `vscode.openWith` cannot: its resolver only reuses a tab when the editor type
 * matches, so opening a file's preview over its source leaves the source tab
 * sitting behind it. Replacing keeps the tab's place in the tab bar and hands
 * the unsaved changes over — closing the source instead would ask to save them.
 */
const REOPEN_ACTIVE_EDITOR_WITH = 'reopenActiveEditorWith';

/**
 * Opens a markdown file *straight into* the preview: no text editor is created
 * first, so there is no flash of source and no tab switch on open. Switching to
 * Preview mode lands here, and it can also own the file from the moment it is
 * opened:
 *
 * ```jsonc
 * "workbench.editorAssociations": { "*.md": "markdownPreviewUltra.editor" }
 * ```
 *
 * VSCode owns these webviews, one per tab, and binds each to its document for
 * the tab's life. So unlike the following panel there is nothing to retarget:
 * no active-editor following, no pinning, and no link history — a link opens
 * its own tab. What this surface does have is a way back to the text editor:
 * the Edit/Split buttons and a double-click both hand off to it.
 */
export class MarkdownEditorProvider implements vscode.CustomTextEditorProvider {
  public static readonly viewType = 'markdownPreviewUltra.editor';

  /**
   * Where each file's preview stands, by URI. A mode switch *replaces* the tab
   * rather than moving it, so the line being read has to outlive the webview
   * that reported it — parked on the way in, taken on the way out. One-shot:
   * whoever takes it owns it.
   */
  private readonly parkedLines = new Map<string, number>();

  constructor(private readonly renderer: PreviewRenderer) {}

  /** Hand the preview editor a line to open at; set before `vscode.openWith`. */
  public parkLine(uri: vscode.Uri, line: number): void {
    this.parkedLines.set(uri.toString(), line);
  }

  /** Take the line this file's preview was last read to, if it reported one. */
  public takeLine(uri: vscode.Uri): number | undefined {
    const key = uri.toString();
    const line = this.parkedLines.get(key);
    this.parkedLines.delete(key);
    return line;
  }

  public resolveCustomTextEditor(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
    _token: vscode.CancellationToken,
  ): void {
    panel.webview.options = {
      enableScripts: true,
      localResourceRoots: this.renderer.localResourceRoots(document),
    };
    panel.webview.html = this.renderer.html(panel.webview);

    let session: EngineSession | null = null;
    let debounce: ReturnType<typeof setTimeout> | undefined;
    let visible = panel.visible;

    const update = (): void => {
      session ??= this.renderer.createSession();
      this.renderer.update(panel.webview, document, session, NO_HISTORY);
    };

    const disposables = [
      panel.webview.onDidReceiveMessage((msg: unknown) => {
        if (isWebviewToHost(msg)) {
          void this.onMessage(document, panel, msg, update, () => {
            // Escape hatch after a webview-side failure: rebuild from a clean
            // baseline rather than patching onto a DOM we no longer trust.
            session?.dispose();
            session = null;
            update();
          });
        }
      }),
      vscode.workspace.onDidChangeTextDocument((e) => {
        if (e.document.uri.toString() !== document.uri.toString()) return;
        if (debounce) clearTimeout(debounce);
        debounce = setTimeout(() => {
          debounce = undefined;
          update();
        }, DEBOUNCE_MS);
      }),
      vscode.workspace.onDidChangeConfiguration((e) => {
        // Engine options are compared WASM-side; a change forces `reset: true`.
        if (e.affectsConfiguration('markdownPreviewUltra')) update();
      }),
      vscode.window.onDidChangeActiveColorTheme((theme) =>
        this.renderer.postTheme(panel.webview, theme),
      ),
      panel.onDidChangeViewState(() => {
        // A hidden webview is kept alive but has no layout; the page stops
        // measuring until it hears it is back.
        if (panel.visible === visible) return;
        visible = panel.visible;
        this.renderer.post(panel.webview, { type: 'visibility', visible });
      }),
    ];

    panel.onDidDispose(() => {
      if (debounce) clearTimeout(debounce);
      for (const d of disposables) d.dispose();
      session?.dispose();
      // A mode switch takes the line before it replaces the tab; anything left
      // here belongs to a tab the reader simply closed.
      this.parkedLines.delete(document.uri.toString());
    });
  }

  private async onMessage(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
    msg: WebviewToHost,
    update: () => void,
    reset: () => void,
  ): Promise<void> {
    switch (msg.type) {
      case 'ready': {
        // The tab can be restored behind another one; tell the page where it
        // stands before the first render so it knows not to measure.
        this.renderer.post(panel.webview, {
          type: 'visibility',
          visible: panel.visible,
        });
        update();
        // Arriving from the text editor: pick the reader up where the source
        // left off, rather than at the top of the file. The patch above is
        // applied first, so the page has something to scroll through.
        const line = this.takeLine(document.uri);
        if (line !== undefined && this.renderer.readSettings().scrollSync) {
          this.renderer.post(panel.webview, { type: 'scroll', line, ratio: 0 });
        }
        break;
      }
      case 'revealLine': {
        if (!this.renderer.readSettings().scrollSync) return;
        // Remembered even with no editor to reveal in: this is the position a
        // switch back to Edit hands over.
        this.parkLine(document.uri, msg.line);
        // Revealing is only meaningful once the source is split out beside us.
        const editor = visibleEditorFor(document);
        editor?.revealRange(
          new vscode.Range(msg.line, 0, msg.line, 0),
          vscode.TextEditorRevealType.AtTop,
        );
        break;
      }
      case 'jumpToLine':
        // Double-click on the page: this tab *is* the preview, so the source
        // has nowhere to go but beside it.
        await openSource(document, vscode.ViewColumn.Beside, {
          line: msg.line,
          select: true,
        });
        break;
      case 'navigate':
        // No history: the tab is bound to its document (buttons stay hidden).
        break;
      case 'openLink':
        await this.openLink(document, panel, msg.href);
        break;
      case 'toggleTask':
        await applyTaskToggle(document, msg);
        break;
      case 'error':
        console.error(
          `markdown-preview-ultra webview error [${msg.context}]: ${msg.message}`,
        );
        reset();
        break;
    }
  }

  private async openLink(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
    href: string,
  ): Promise<void> {
    try {
      if (/^(https?|mailto):/i.test(href)) {
        await vscode.env.openExternal(vscode.Uri.parse(href));
        return;
      }
      const target = resolveLink(document, href);
      // Browsing to another markdown file opens its own preview tab (the
      // association routes it back here), the way following a link in a
      // browser opens a page, not an editor.
      await vscode.commands.executeCommand('vscode.open', target, {
        viewColumn: panel.viewColumn,
      });
    } catch (err) {
      vscode.window.showErrorMessage(
        `Could not open link: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  }
}

/** A visible text editor showing `document`, if one is on screen. */
function visibleEditorFor(
  document: vscode.TextDocument,
): vscode.TextEditor | undefined {
  const uri = document.uri.toString();
  return vscode.window.visibleTextEditors.find(
    (editor) => editor.document.uri.toString() === uri,
  );
}

/** What a tab is showing, for the two kinds of tab this extension opens. */
function tabEditor(
  tab: vscode.Tab,
): { uri: vscode.Uri; editorId: string } | undefined {
  const input = tab.input;
  if (input instanceof vscode.TabInputText) {
    return { uri: input.uri, editorId: TEXT_EDITOR };
  }
  if (input instanceof vscode.TabInputCustom) {
    return { uri: input.uri, editorId: input.viewType };
  }
  return undefined;
}

/**
 * The editor a tab in `column` is using to show `uri`, if one is. The active tab
 * wins, so a mode switch acts on the tab the reader is looking at.
 */
function editorShowing(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
): string | undefined {
  const group = vscode.window.tabGroups.all.find(
    (candidate) => candidate.viewColumn === column,
  );
  if (!group) return undefined;
  const tabs = group.activeTab ? [group.activeTab, ...group.tabs] : group.tabs;
  for (const tab of tabs) {
    const shown = tabEditor(tab);
    if (shown?.uri.toString() === uri.toString()) return shown.editorId;
  }
  return undefined;
}

/**
 * Show `uri` in `column` under the editor `editorId`, taking over the tab that
 * already shows the file rather than opening in front of it.
 *
 * The symbolic columns are left to VSCode: `Beside` and `Active` match no group,
 * so they open a tab of their own the way a jump to the source should.
 */
async function showWith(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
  editorId: string,
): Promise<void> {
  const current = editorShowing(uri, column);
  if (current !== undefined && current !== editorId) {
    // Reopen With acts on the active editor, so the tab has to come forward as
    // it stands before VSCode is asked to swap what is inside it.
    await openWith(uri, column, current);
    await vscode.commands.executeCommand(REOPEN_ACTIVE_EDITOR_WITH, editorId);
    return;
  }
  // Nothing to take over, or the tab already holds the right editor — which
  // makes this a reveal rather than a second copy.
  await openWith(uri, column, editorId);
}

function openWith(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
  editorId: string,
): Thenable<unknown> {
  return vscode.commands.executeCommand('vscode.openWith', uri, editorId, {
    viewColumn: column,
    preserveFocus: false,
  });
}

/**
 * Open the *source* of a markdown file. Plain `vscode.open` would be routed
 * straight back to the preview by the editor association, so the text editor
 * has to be named explicitly.
 */
export async function openSource(
  uriOrDocument: vscode.Uri | vscode.TextDocument,
  column: vscode.ViewColumn,
  reveal?: {
    line: number;
    /** Put the cursor there too — a jump to the source, not a mode switch. */
    select?: boolean;
  },
): Promise<void> {
  const uri = 'uri' in uriOrDocument ? uriOrDocument.uri : uriOrDocument;
  await showWith(uri, column, TEXT_EDITOR);
  if (!reveal) return;
  const editor = vscode.window.visibleTextEditors.find(
    (ed) => ed.document.uri.toString() === uri.toString(),
  );
  if (!editor) return;
  const target = new vscode.Range(reveal.line, 0, reveal.line, 0);
  if (reveal.select) {
    editor.selection = new vscode.Selection(target.start, target.start);
  }
  editor.revealRange(target, vscode.TextEditorRevealType.AtTop);
}

/**
 * Open a markdown file *in* the preview editor — the mirror of `openSource`.
 * The tab holding the source becomes the preview, rather than gaining a second
 * tab in front of it.
 */
export async function openPreviewEditor(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
): Promise<void> {
  await showWith(uri, column, MarkdownEditorProvider.viewType);
}

/**
 * The file shown by the active tab, when that tab is a preview editor. Drives
 * the mode state machine: with a custom editor active there is no
 * `activeTextEditor` to read the current document from.
 */
export function activePreviewEditorUri(): vscode.Uri | undefined {
  const input = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
  if (
    input instanceof vscode.TabInputCustom &&
    input.viewType === MarkdownEditorProvider.viewType
  ) {
    return input.uri;
  }
  return undefined;
}

/** Whether this extension can open `uri` as a preview editor. */
export function isPreviewable(uri: vscode.Uri): boolean {
  return isMarkdownPath(uri.fsPath);
}
