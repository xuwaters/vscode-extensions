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
 *
 * Lifted from markdown-preview-ultra, whose model this deliberately matches:
 * two extensions in the same repo that both put a document beside its source
 * should not disagree about what `cmd+shift+v` means.
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

/** Edit → Split → Preview → Edit. */
export const CYCLE: Record<PreviewMode, PreviewMode> = {
  edit: 'split',
  split: 'preview',
  preview: 'edit',
};

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
