/**
 * The three view modes and the rule that derives one from observable layout.
 *
 * Kept free of the `vscode` module so the rule itself is testable: columns are
 * plain numbers (`ViewColumn` is a numeric enum).
 *
 * - `edit`    — no preview on screen.
 * - `preview` — the preview occupies the source editor's column, or *is* the
 *               tab (the custom editor, which has no source editor at all).
 * - `split`   — the panel lives in another column.
 */
export type PreviewMode = 'edit' | 'split' | 'preview';

export interface PreviewPlacement {
  /** A preview-editor tab is the active tab: the file opened straight into it. */
  previewEditorActive: boolean;
  /** The following panel exists. */
  hasPanel: boolean;
  /** Column the panel occupies. */
  panelColumn?: number;
  /** Column the panel's source editor lives in. */
  sourceColumn?: number;
}

/**
 * Where one key pressed twice lands: Edit ⇄ Preview. Split is a way of showing
 * the source, so it toggles *away* from the editor like Edit does — the key
 * always means "show me the other one".
 */
export function toggleEditPreview(current: PreviewMode): PreviewMode {
  return current === 'preview' ? 'edit' : 'preview';
}

export function resolveMode(placement: PreviewPlacement): PreviewMode {
  // A preview-editor tab is the whole view — there is no source editor beside
  // it to be split from, so it reads as Preview regardless of any panel that
  // may also be open elsewhere in the window.
  if (placement.previewEditorActive) return 'preview';
  if (!placement.hasPanel) return 'edit';
  const { panelColumn, sourceColumn } = placement;
  return panelColumn !== undefined && panelColumn === sourceColumn
    ? 'preview'
    : 'split';
}

/**
 * Whether a preview-editor tab that has just opened in `tabColumn` should be
 * handed straight back to the text editor.
 *
 * With `workbench.editorAssociations` pointing `*.md` at the preview editor,
 * *every* way of opening a markdown file goes straight there — including
 * clicking the next file while reading in Split. But Split is a layout the
 * reader chose:
 * source in one column, preview beside it. A file arriving in the source column
 * is a request for that file, not for a different layout, so the column keeps
 * showing source and the panel — which follows the active editor — picks the
 * new file up. Opening it as a preview tab instead would bury the source and
 * leave the panel behind on the old file.
 *
 * The tab being decided is excluded from the layout it is judged against: it is
 * active before this extension hears about it, and reading it back would answer
 * "Preview" every time. Only the source column is claimed — a preview tab that
 * lands anywhere else is not standing in the source's place.
 */
export function shouldHandOffToSource(
  placement: Omit<PreviewPlacement, 'previewEditorActive'>,
  tabColumn: number,
): boolean {
  const mode = resolveMode({ ...placement, previewEditorActive: false });
  return mode === 'split' && tabColumn === placement.sourceColumn;
}

/** A markdown file that has just been opened in the text editor. */
export interface OpenedTab {
  /** Column the editor landed in. */
  column: number;
  /**
   * The editor arrived with text selected. VSCode selects what it took you to,
   * so this is the mark of a file opened *at* something rather than just
   * opened: a search result, Go to Definition, a reference, a problem. A plain
   * open leaves an empty cursor, as does a restored one.
   */
  hasSelection: boolean;
}

/**
 * Whether a markdown file that has just opened as text should be handed over to
 * the preview editor.
 *
 * Reading markdown means reading it rendered, so the ordinary open goes to the
 * preview. Two things say otherwise.
 *
 * A file opened at a place was opened *for* that place — the reader is chasing
 * a keyword through search results, or a symbol through its references — and
 * the place is in the source text. The preview cannot show it: VSCode passes
 * the match as an editor option and drops it on the way into a custom editor,
 * so the page would open at the top of the file with nothing to say what was
 * being looked for. That is the reader's own click answered with less than they
 * asked for, so those tabs stay on the source, where VSCode has already put the
 * cursor on the match.
 *
 * And a reader in Split has already said where the source goes: the same claim
 * `shouldHandOffToSource` makes on behalf of the source column, made from the
 * other side.
 */
export function shouldPromoteToPreview(
  placement: Omit<PreviewPlacement, 'previewEditorActive'>,
  tab: OpenedTab,
  previewByDefault: boolean,
): boolean {
  if (!previewByDefault) return false;
  if (tab.hasSelection) return false;
  return !shouldHandOffToSource(placement, tab.column);
}
