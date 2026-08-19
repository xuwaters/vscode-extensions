/**
 * Ordering for the question "which of these files is the document?".
 *
 * Nothing here reads the file system or the files themselves. The include
 * graph would be the honest answer and the server has one, but every caller of
 * this is filling a list the reader is about to read anyway: floating the
 * likely answer to the top is worth a heuristic, getting it provably right is
 * not.
 *
 * Kept free of the `vscode` module — callers hand over workspace-relative
 * paths and map the answer back — so the rule itself is testable.
 */

/** Names projects give their entry point, often enough to bet on. */
const CONVENTIONAL =
  /^(main|index|paper|thesis|report|book|article|slides|presentation|cv|resume)\.typc?$/i;

/**
 * Sort likely entry points first.
 *
 * A conventional name outranks everything, because a project that has a
 * `main.typ` has already answered the question. Shallow beats deep next: an
 * entry point sits above the chapters it pulls in, not beside them. Ties break
 * alphabetically so the same list twice is the same list.
 */
export function rankEntryCandidates(paths: readonly string[]): string[] {
  return [...paths].sort(
    (a, b) =>
      named(a) - named(b) || depth(a) - depth(b) || (a < b ? -1 : a > b ? 1 : 0),
  );
}

/** 0 for a conventional entry-point name, 1 for anything else. */
function named(candidate: string): number {
  return CONVENTIONAL.test(basename(candidate)) ? 0 : 1;
}

/** How many folders deep, counting either separator: this also runs on Windows. */
function depth(candidate: string): number {
  return segments(candidate).length - 1;
}

function basename(candidate: string): string {
  const parts = segments(candidate);
  return parts[parts.length - 1] ?? '';
}

function segments(candidate: string): string[] {
  return candidate.split(/[\\/]+/).filter((part) => part.length > 0);
}
