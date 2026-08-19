import type * as vscode from 'vscode';
import { parsePreviewPlace, type PreviewPlace } from './messages.js';

/** Where the reader's preview setup is kept between surfaces. */
const KEY = 'typstUltra.previewState';

/** Where the panel's subject is kept between windows. */
const SUBJECT_KEY = 'typstUltra.previewSubject';

/** What the panel was showing, and how, when the window went away. */
export interface PreviewSubject {
  /** The document the panel was showing, as a URI string. */
  uri: string;
  /** Whether the reader had pinned it there. */
  locked: boolean;
}

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

/**
 * What the panel was showing, if we know.
 *
 * VSCode brings the webview back after a reload, but only the webview: the
 * state it carries across is the page's own — zoom, fit, inversion — and which
 * document the panel was *for* is the host's half of the arrangement, which the
 * host has just lost by restarting. Asking the active editor is not an answer,
 * because a panel with the focus in it makes no editor active, and because a
 * webview is restored early enough in a startup that the editors beside it may
 * not be back yet. So the subject is written down as it changes, and read here.
 *
 * Never cleared: it is only ever consulted while a panel is being restored, and
 * a panel being restored needs a subject more than it needs a fresh one.
 */
export function readSubject(
  context: vscode.ExtensionContext,
): PreviewSubject | undefined {
  const stored: unknown = context.workspaceState.get(SUBJECT_KEY);
  if (typeof stored !== 'object' || stored === null) return undefined;
  const { uri, locked } = stored as Partial<PreviewSubject>;
  if (typeof uri !== 'string' || uri.length === 0) return undefined;
  return { uri, locked: locked === true };
}

/** Remember what the panel is showing, so a reload can put it back. */
export async function writeSubject(
  context: vscode.ExtensionContext,
  subject: PreviewSubject,
): Promise<void> {
  await context.workspaceState.update(SUBJECT_KEY, subject);
}
