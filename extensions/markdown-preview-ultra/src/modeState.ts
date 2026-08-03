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
 * *every* way of opening a markdown file goes there — including clicking the
 * next file while reading in Split. But Split is a layout the reader chose:
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
