/**
 * Which document the panel shows — when the focus moves, and when the panel
 * comes back from a window reload with nothing but its own tab.
 *
 * One shared panel following the focus is right for a folder of standalone
 * documents and wrong for a project: clicking `data.typ` in a paper that
 * `#include`s it retargets the preview at a file of `#let` bindings, which
 * compiles to zero pages and shows as a blank tab. A compile root is the
 * reader saying which file *is* the document, so once one is set the panel
 * follows it rather than the focus, and a panel that is somehow showing
 * something else comes back to it.
 *
 * Kept free of the `vscode` module — URIs arrive as strings — so the rule
 * itself is testable.
 */
export type FollowAction =
  /** Leave the panel showing what it is showing. */
  | 'stay'
  /** Retarget to the file that just became active. */
  | 'showActive'
  /** Retarget to the compile root. */
  | 'showEntry';

export interface FollowInputs {
  /** The `.typ` document that just became active, as a URI string. */
  active: string;
  /** What the panel is showing now, if it has a subject yet. */
  target: string | undefined;
  /** The pinned or configured compile root, if one is set. */
  entry: string | undefined;
  /** The reader has pinned the panel to its document. */
  locked: boolean;
}

/** What a panel coming back from a reload has to choose a subject from. */
export interface RestoreInputs {
  /** The pinned or configured compile root, if one is set. */
  entry: string | undefined;
  /** What the panel was showing before the window went away. */
  remembered: string | undefined;
  /** The `.typ` the reader is in — `undefined` this early in a startup. */
  active: string | undefined;
  /** Every typst file the window has open, in tab order. */
  open: readonly string[];
}

/**
 * Which document a restored panel should show.
 *
 * The compile root leads for the same reason it does in `show`: with one
 * settled, that file *is* the document. After it comes the panel's own last
 * subject, which is the only one of these that describes the panel rather than
 * the window — it outranks the focus because a pinned panel must come back
 * pinned to what it was pinned to, and an unpinned one is corrected by the
 * first `decideFollow` anyway. The two editor answers are the fallback for a
 * panel restored before this extension ever wrote a subject down.
 */
export function chooseSubject(inputs: RestoreInputs): string | undefined {
  return inputs.entry ?? inputs.remembered ?? inputs.active ?? inputs.open[0];
}

export function decideFollow(inputs: FollowInputs): FollowAction {
  // A lock is the reader overruling every rule below, including the root.
  if (inputs.locked) return 'stay';

  if (inputs.entry !== undefined) {
    return inputs.target === inputs.entry ? 'stay' : 'showEntry';
  }

  return inputs.target === inputs.active ? 'stay' : 'showActive';
}
