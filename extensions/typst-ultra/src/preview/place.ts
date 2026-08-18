import type * as vscode from 'vscode';
import { parsePreviewPlace, type PreviewPlace } from './messages.js';

/** Where the reader's preview setup is kept between surfaces. */
const KEY = 'typstUltra.previewState';

/**
 * How the preview was last set up, if we know.
 *
 * Workspace-scoped rather than global: a fit that suits the two-column paper
 * in one project is not the one that suits the slides in another, and the
 * panel's width is a property of the window it was arranged in.
 *
 * Validated on the way out. The value is our own, but it is a file on disk that
 * an older version of this extension — or a hand edit — may have left in a
 * shape this one does not accept, and a bad zoom would size every page from it.
 */
export function readPlace(context: vscode.ExtensionContext): PreviewPlace | undefined {
  return parsePreviewPlace(context.workspaceState.get(KEY)) ?? undefined;
}

/** Remember how the reader has the preview set up. */
export async function writePlace(
  context: vscode.ExtensionContext,
  place: PreviewPlace,
): Promise<void> {
  await context.workspaceState.update(KEY, place);
}
