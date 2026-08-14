import * as vscode from 'vscode';
import { applyTaskToggle, resolveLink } from './actions';
import type { EngineSession } from './engine';
import { isWebviewToHost, type WebviewToHost } from './messages';
import { shouldHandOffToSource } from './modeState';
import type { PreviewManager } from './previewManager';
import type { PreviewPromoter } from './promote';
import { NO_HISTORY, PreviewRenderer } from './renderer';
import { firstSearchMatch, type SearchMatch } from './searchMatches';
import { isPreviewEditorPath, visibleEditorFor } from './util';

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
 * The search view's own results, as text. Not API — see `searchMatches.ts`.
 */
const GET_SEARCH_RESULTS = 'search.action.getSearchResults';

/**
 * Opens a markdown file *straight into* the preview. Switching to Preview mode
 * lands here, as does the promoter that hands newly-opened markdown over.
 *
 * A reader can also point VSCode's own editor association at this editor:
 *
 * ```jsonc
 * "workbench.editorAssociations": { "*.md": "markdownPreviewUltra.editor" }
 * ```
 *
 * which opens the preview with no flash of source at all, since no text editor
 * is created first. The extension does not set that itself — it routes *every*
 * markdown file here, including two that do not belong: a reader in Split has
 * already said where the source goes (`handOffToSource`), and a search result
 * names a match VSCode drops on the way in (`revealSearchMatch`).
 *
 * VSCode owns these webviews, one per tab, and binds each to its document for
 * the tab's life. So unlike the following panel there is nothing to retarget:
 * no active-editor following, no pinning, and no link history — a link opens
 * its own tab. The way back to the text editor is a mode switch, which hands
 * this tab over to it; nothing the reader does *inside* the page opens one.
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

  /**
   * Files a hand-off back to the source is already in flight for. Taking a tab
   * over reveals it first, which can route back through this provider for the
   * same file; the second pass renders rather than bouncing again.
   */
  private readonly handingOff = new Set<string>();

  constructor(
    private readonly renderer: PreviewRenderer,
    private readonly manager: PreviewManager,
    private readonly promoter: PreviewPromoter,
  ) {}

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
    // The editor association routes every markdown file here, Split mode
    // included — where the tab belongs to the source. Nothing is wired up for a
    // tab that is on its way out.
    if (this.handOffToSource(document, panel)) return;
    // Anything this extension did not open itself was opened by that
    // association, which is the one path a search result can arrive down.
    if (!takeDeliberate(document.uri)) this.revealSearchMatch(document, panel);

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
      // The light/dark switch belongs to the window, not to this tab: flipping
      // it in any preview restyles this one too, and the next file opened comes
      // up already flipped.
      this.renderer.onDidChangeThemeOverride(() =>
        this.renderer.postThemeOverride(panel.webview),
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

  /**
   * Give the tab back to the text editor when it has landed in the source
   * column of a split, and report having done so.
   *
   * Only tabs that *arrived* here are handed back: a mode switch into Preview
   * closes the panel before opening this editor, so by the time it reaches us
   * there is no split left to hand back to. Anything still standing in a split
   * came from the editor association, which is to say from the reader opening a
   * file rather than asking for a different layout.
   */
  private handOffToSource(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
  ): boolean {
    const key = document.uri.toString();
    if (this.handingOff.has(key)) return false;
    const column = panel.viewColumn;
    if (column === undefined) return false;
    const claim = shouldHandOffToSource(
      {
        hasPanel: this.manager.hasPreview,
        panelColumn: this.manager.panelColumn,
        sourceColumn: this.manager.sourceColumn,
      },
      column,
    );
    if (!claim) return false;
    this.handingOff.add(key);
    this.promoter.settle(document.uri);
    // This tab is still being resolved; swapping what is inside it from in here
    // would re-enter the editor service, so let the resolve return first.
    setTimeout(() => {
      // The panel follows the active editor, so retargeting it to the new file
      // is the source editor taking focus — nothing more to do here.
      void openSource(document.uri, column).then(
        () => this.handingOff.delete(key),
        (err: unknown) => {
          this.handingOff.delete(key);
          console.error(
            'markdown-preview-ultra: hand-off to source failed',
            err,
          );
        },
      );
    }, 0);
    return true;
  }

  /**
   * Give the tab back to the text editor, on the match, when this file was
   * opened from the search view.
   *
   * Only reached with the editor association in play, because that is the only
   * way a search result gets here — and, just as importantly, because the
   * results it reads carry no clock. They are whatever the search view is still
   * showing, which may be a query from an hour ago that happens to name this
   * file; against that the association is the standing evidence that the reader
   * did not choose this preview, file by file, themselves. What the results do
   * carry is the line as it read when they were found, so a result the file has
   * since moved past is dropped rather than jumped to.
   *
   * The search runs while the page renders behind it: an answer that never
   * comes, or comes back empty, costs a reader who was not searching nothing.
   */
  private revealSearchMatch(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
  ): void {
    const column = panel.viewColumn;
    if (column === undefined) return;
    const key = document.uri.toString();
    if (this.handingOff.has(key)) return;
    if (!associationOpensPreview()) return;
    this.handingOff.add(key);
    let closed = false;
    const watch = panel.onDidDispose(() => {
      closed = true;
    });
    void searchMatchIn(document)
      .then(async (match) => {
        // The tab can be shut while the search is still answering. Opening a
        // file the reader has just closed is worse than not answering at all.
        if (!match || closed) return;
        this.promoter.settle(document.uri);
        await openSource(document.uri, column, match);
      })
      .catch((err: unknown) => {
        console.error('markdown-preview-ultra: search reveal failed', err);
      })
      .finally(() => {
        watch.dispose();
        this.handingOff.delete(key);
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
          // The page stands here now, so this is also where a switch back
          // hands over — a reader who opens the preview and switches straight
          // to Split has reported no position of their own to use instead.
          this.parkLine(document.uri, line);
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
      case 'jumpToLine': {
        // Double-click on the page means "show me that bit of the source" —
        // not "give me a source to show it in". With the file already split
        // out beside us there is an editor to put on the line; with none, the
        // reader is looking at a preview, and splitting the layout out from
        // under a double-click is the wrong kind of helpful.
        const editor = visibleEditorFor(document);
        if (!editor) break;
        const target = new vscode.Range(msg.line, 0, msg.line, 0);
        editor.selection = new vscode.Selection(target.start, target.start);
        editor.revealRange(target, vscode.TextEditorRevealType.AtTop);
        break;
      }
      case 'openSource': {
        // The toolbar's Edit button, which is the mode switch out of Preview
        // asked for from the page: the source takes this tab over — keeping its
        // place in the tab bar and its unsaved changes — and opens on the
        // passage being read. Acting on this panel's own document rather than
        // on whatever is active is what the click actually meant.
        const line = this.takeLine(document.uri);
        this.promoter.settle(document.uri);
        await openSource(
          document.uri,
          panel.viewColumn ?? vscode.ViewColumn.One,
          line === undefined ? undefined : { line },
        );
        break;
      }
      case 'navigate':
        // No history: the tab is bound to its document (buttons stay hidden).
        break;
      case 'openLink':
        await this.openLink(document, panel, msg.href);
        break;
      case 'setTheme':
        this.renderer.setThemeOverride(msg.theme);
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
      // Browsing to another markdown file opens its own preview tab, the way
      // following a link in a browser opens a page, not an editor. Named
      // outright rather than left to the promoter: a link followed from a page
      // never wanted the source, not even for the moment it would show.
      if (isPreviewEditorPath(target.fsPath)) {
        const column = panel.viewColumn ?? vscode.ViewColumn.One;
        await openPreviewEditor(target, column);
        return;
      }
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

/**
 * Files this extension is opening as a preview itself, so that the resolve they
 * cause is not mistaken for the editor association opening one. One-shot, like
 * `parkedLines`: whoever reads it owns it, and a resolve that never comes is
 * cleared by the open that was waiting on it.
 */
const deliberate = new Set<string>();

function takeDeliberate(uri: vscode.Uri): boolean {
  return deliberate.delete(uri.toString());
}

/**
 * Whether the reader has pointed VSCode's editor association at this editor, in
 * which case markdown files reach it without passing through the promoter — and
 * search results reach it with their match already discarded.
 */
function associationOpensPreview(): boolean {
  const associations = vscode.workspace
    .getConfiguration('workbench')
    .get<Record<string, string>>('editorAssociations', {});
  return Object.values(associations).includes(MarkdownEditorProvider.viewType);
}

/** The match in `document` the search view is still showing, if it has one. */
async function searchMatchIn(
  document: vscode.TextDocument,
): Promise<SearchMatch | undefined> {
  let results: unknown;
  try {
    results = await vscode.commands.executeCommand(GET_SEARCH_RESULTS);
  } catch {
    // Not API: a VSCode that no longer offers this leaves the preview as it is.
    return undefined;
  }
  if (typeof results !== 'string') return undefined;
  const match = firstSearchMatch(results, document.uri.fsPath);
  if (!match || match.line >= document.lineCount) return undefined;
  // Results outlive the file they describe. The line the search recorded is
  // compared against the line as it stands — allowing for a preview the search
  // view truncated, and for one it trimmed.
  const actual = document.lineAt(match.line).text;
  const stands =
    actual.startsWith(match.text) ||
    actual.trimStart().startsWith(match.text.trimStart());
  return stands ? match : undefined;
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
 * Whether `uri` is what the active tab of the active group is already showing.
 *
 * Reopen With acts on the active editor, and inherits the options that editor
 * was opened with — including whether opening it took focus. So a tab that is
 * already in front is best left exactly as it is: restating the open would
 * overwrite those options with this extension's own.
 */
function isActiveTab(uri: vscode.Uri, column: vscode.ViewColumn): boolean {
  const group = vscode.window.tabGroups.activeTabGroup;
  if (group.viewColumn !== column) return false;
  const active = group.activeTab;
  return (
    active !== undefined &&
    tabEditor(active)?.uri.toString() === uri.toString()
  );
}

/**
 * Show `uri` in `column` under the editor `editorId`, taking over the tab that
 * already shows the file rather than opening in front of it.
 *
 * The symbolic columns are left to VSCode: `Beside` and `Active` match no group,
 * so they open a tab of their own the way a jump to the source should.
 *
 * `inPlace` says the tab is already where the reader is looking and only its
 * contents are to change — see `openPreviewEditor`.
 */
async function showWith(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
  editorId: string,
  inPlace = false,
): Promise<void> {
  const current = editorShowing(uri, column);
  if (current !== undefined && current !== editorId) {
    // Reopen With acts on the active editor, so the tab has to come forward as
    // it stands before VSCode is asked to swap what is inside it — unless it is
    // already in front and the caller asked for it to be left that way.
    if (!(inPlace && isActiveTab(uri, column))) {
      await openWith(uri, column, current);
    }
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
  reveal?: { line: number; column?: number },
): Promise<void> {
  const uri = 'uri' in uriOrDocument ? uriOrDocument.uri : uriOrDocument;
  await showWith(uri, column, TEXT_EDITOR);
  if (reveal === undefined) return;
  const editor = vscode.window.visibleTextEditors.find(
    (ed) => ed.document.uri.toString() === uri.toString(),
  );
  if (!editor) return;
  if (reveal.column === undefined) {
    // Carrying a reader across a mode switch: the passage they were on belongs
    // at the top of the editor, where it was at the top of the page.
    editor.revealRange(
      new vscode.Range(reveal.line, 0, reveal.line, 0),
      vscode.TextEditorRevealType.AtTop,
    );
    return;
  }
  // Landing on a search match, so the cursor goes on it and it is centred the
  // way the search view would have centred it. Only the start of the match is
  // in the results, so there is nothing to select — the caret marks the spot.
  const at = new vscode.Position(reveal.line, reveal.column);
  editor.selection = new vscode.Selection(at, at);
  editor.revealRange(
    new vscode.Range(at, at),
    vscode.TextEditorRevealType.InCenterIfOutsideViewport,
  );
}

/**
 * Open a markdown file *in* the preview editor — the mirror of `openSource`.
 * The tab holding the source becomes the preview, rather than gaining a second
 * tab in front of it.
 *
 * `inPlace` is the promoter's, whose tab has only just opened and is already in
 * front: revealing it first would restate how it was opened in this extension's
 * terms, and Reopen With inherits those terms — so a file the explorer opened
 * without taking focus would have focus taken from it on the way to the
 * preview, and the reader would lose their place in the tree.
 */
export async function openPreviewEditor(
  uri: vscode.Uri,
  column: vscode.ViewColumn,
  inPlace = false,
): Promise<void> {
  const key = uri.toString();
  deliberate.add(key);
  try {
    await showWith(uri, column, MarkdownEditorProvider.viewType, inPlace);
  } finally {
    // Normally taken by the resolve this triggered. A resolve that never came —
    // the tab was already showing the preview, say — leaves it to be cleared
    // here, so a later association-opened tab is not read as this one.
    deliberate.delete(key);
  }
}

/** The file a tab is showing, for the two kinds of tab this extension opens. */
export function tabResource(tab: vscode.Tab): vscode.Uri | undefined {
  return tabEditor(tab)?.uri;
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
  return isPreviewEditorPath(uri.fsPath);
}
