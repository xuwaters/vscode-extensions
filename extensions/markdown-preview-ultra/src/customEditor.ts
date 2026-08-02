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
 * Opens a markdown file *straight into* the preview: no text editor is created
 * first, so there is no flash of source and no tab switch on open. Enable it
 * per-user with
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

  constructor(private readonly renderer: PreviewRenderer) {}

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
      case 'ready':
        // The tab can be restored behind another one; tell the page where it
        // stands before the first render so it knows not to measure.
        this.renderer.post(panel.webview, {
          type: 'visibility',
          visible: panel.visible,
        });
        update();
        break;
      case 'revealLine': {
        // Only meaningful once the source has been split out beside us.
        if (!this.renderer.readSettings().scrollSync) return;
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
        await openSource(document, vscode.ViewColumn.Beside, msg.line);
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

/**
 * Open the *source* of a markdown file. Plain `vscode.open` would be routed
 * straight back to the preview by the editor association, so the text editor
 * has to be named explicitly.
 */
export async function openSource(
  uriOrDocument: vscode.Uri | vscode.TextDocument,
  column: vscode.ViewColumn,
  line?: number,
): Promise<void> {
  const uri = 'uri' in uriOrDocument ? uriOrDocument.uri : uriOrDocument;
  await vscode.commands.executeCommand('vscode.openWith', uri, TEXT_EDITOR, {
    viewColumn: column,
    preserveFocus: false,
  });
  if (line === undefined) return;
  const editor = vscode.window.visibleTextEditors.find(
    (ed) => ed.document.uri.toString() === uri.toString(),
  );
  if (!editor) return;
  const target = new vscode.Range(line, 0, line, 0);
  editor.selection = new vscode.Selection(target.start, target.start);
  editor.revealRange(target, vscode.TextEditorRevealType.AtTop);
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
