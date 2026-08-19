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

/** What the walk below is looking for. */
const DOCUMENT = /\.typc?$/i;

/**
 * Directories a document is never in, so not worth the read on the way to one.
 *
 * Deliberately short. This is the *near* walk, which trades completeness for
 * an answer before the reader has finished reaching for the keyboard; anything
 * it wrongly skips is a keystroke away in the workspace search, which honours
 * `files.exclude` and `search.exclude` properly.
 */
const UNINTERESTING = new Set(['node_modules', 'target', 'dist', 'out', 'build', 'vendor']);

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

/** How far, and how hard, the walk for nearby documents is allowed to look. */
export interface WalkBudget {
  /** Levels of subdirectory below each root. */
  depth: number;
  /** Directories read, counted across every root. */
  dirs: number;
  /** Documents after which the walk has enough to show and stops. */
  files: number;
}

/**
 * Small enough that the walk finishes in the time it takes a menu to animate
 * open, big enough to reach `chapters/` from the file next to it.
 */
export const NEARBY_BUDGET: WalkBudget = { depth: 2, dirs: 24, files: 48 };

/**
 * The directory tree, as little of it as the walk needs to know.
 *
 * An interface rather than `vscode.workspace.fs` directly, so the budget and
 * the skipping below are testable without a workspace, and so the walk works
 * on whatever scheme the reader's files arrived over.
 */
export interface DirectoryTree<T> {
  /** One directory's entries: each a name, and whether it is a directory. */
  read(directory: T): Promise<readonly (readonly [string, boolean])[]>;
  /** The child of a directory with that name. */
  join(directory: T, name: string): T;
  /** A stable identity, so a directory reached from two roots is read once. */
  id(directory: T): string;
}

/**
 * Documents within a short walk of where the reader is, breadth first.
 *
 * Bounded on purpose, and in three ways at once. This runs while the quick
 * pick is already on screen, so the useful property is not "finds everything"
 * — the workspace search does that — but "finishes". A project whose root
 * holds a thousand directories costs the same here as one that holds three.
 *
 * Roots are visited in the order given, so pass the closest first: when the
 * budget runs out, what survives should be what the reader is next to.
 */
export async function walkForDocuments<T>(
  roots: readonly T[],
  tree: DirectoryTree<T>,
  budget: WalkBudget = NEARBY_BUDGET,
): Promise<T[]> {
  const queue = roots.map((directory) => ({ directory, depth: 0 }));
  const seen = new Set(roots.map((directory) => tree.id(directory)));
  const found: T[] = [];

  for (let reads = 0; reads < budget.dirs && found.length < budget.files; reads++) {
    const next = queue.shift();
    if (!next) break;

    let entries: readonly (readonly [string, boolean])[];
    try {
      entries = await tree.read(next.directory);
    } catch {
      // A directory that cannot be read is one we simply do not offer from.
      continue;
    }

    for (const [name, isDirectory] of entries) {
      if (!isDirectory) {
        if (DOCUMENT.test(name)) found.push(tree.join(next.directory, name));
        continue;
      }
      if (next.depth >= budget.depth || !worthWalking(name)) continue;
      const child = tree.join(next.directory, name);
      const id = tree.id(child);
      if (seen.has(id)) continue;
      seen.add(id);
      queue.push({ directory: child, depth: next.depth + 1 });
    }
  }

  return found;
}

/** Dot directories are tooling; the rest is the deny list above. */
function worthWalking(name: string): boolean {
  return !name.startsWith('.') && !UNINTERESTING.has(name.toLowerCase());
}

/**
 * The `findFiles` include pattern for what the reader has typed so far.
 *
 * The filtering the quick pick does itself is over the items it already has,
 * which is why the query has to reach the search service at all: the file the
 * reader is describing may not be in the list yet. Only the last path segment
 * goes into the glob — `*` does not cross a separator, so a typed
 * `chapters/one` searches for `one` and lets the quick pick's own matching
 * apply the `chapters/` part to the results.
 */
export function documentSearchGlob(query: string): string {
  const parts = query.split(/[\\/\s]+/).filter((part) => part.length > 0);
  const last = parts[parts.length - 1] ?? '';
  // Glob metacharacters in a filename query are typos, not intent, and `{` in
  // particular would change what the rest of the pattern means.
  const stem = withoutHalfTypedExtension(last).replace(/[*?{}[\]]/g, '');
  return stem ? `**/*${stem}*.{typ,typc}` : '**/*.{typ,typc}';
}

/**
 * Drop a trailing `.t`, `.ty`, `.typ` or `.typc`, which the pattern supplies.
 *
 * Someone typing `main.typ` means the file `main.typ`, but `*main.typ*.typ` is
 * a pattern that matches nothing, and a search that empties the list the
 * moment the reader types the extension looks broken.
 */
function withoutHalfTypedExtension(query: string): string {
  const dot = query.lastIndexOf('.');
  if (dot <= 0) return query;
  return /^t(y(pc?)?)?$/i.test(query.slice(dot + 1)) ? query.slice(0, dot) : query;
}
