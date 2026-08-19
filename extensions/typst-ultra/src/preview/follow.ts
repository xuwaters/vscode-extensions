/**
 * What an active-editor change means for the panel's subject.
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

export function decideFollow(inputs: FollowInputs): FollowAction {
  // A lock is the reader overruling every rule below, including the root.
  if (inputs.locked) return 'stay';

  if (inputs.entry !== undefined) {
    return inputs.target === inputs.entry ? 'stay' : 'showEntry';
  }

  return inputs.target === inputs.active ? 'stay' : 'showActive';
}
