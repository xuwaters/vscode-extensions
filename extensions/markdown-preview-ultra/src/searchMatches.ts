/**
 * Recovering the match behind a search result, for the one configuration that
 * loses it.
 *
 * The search view opens a result by asking for the file *and* the range to
 * reveal. VSCode drops that range when the editor it resolves to is a custom
 * editor, so with `workbench.editorAssociations` pointing markdown at the
 * preview, clicking a result renders the page from the top and never says where
 * the keyword was. Nothing in the extension API reports the query, the match,
 * or even that the open came from search.
 *
 * One thing does report the match: `search.action.getSearchResults`, the
 * command behind the search view's own Copy All. It is not API — it is read
 * defensively, and a parse that finds nothing simply leaves the preview alone.
 * Its text is one block per file, blank-line separated:
 *
 * ```text
 * docs/guide.md
 *   12,5: the line that matched
 *   40,1: another one, and one that
 *   41:   spans two lines
 * ```
 *
 * The path is a workspace label rather than a URI, line and column are 1-based,
 * and the text after the colon is the source line as it stood when the search
 * ran — which is what makes a hit from a search since overtaken detectable.
 */

/** Where the search view says a file matched. */
export interface SearchMatch {
  /** Zero-based, as the extension API counts lines. */
  line: number;
  /** Zero-based column of the start of the match. */
  column: number;
  /**
   * The matching source line as the search recorded it. Results outlive the
   * file they describe, so this is the caller's way to check they still do.
   */
  text: string;
}

/** `  12,5: text` — the first line of a match, the only one that is placed. */
const MATCH = /^ +(\d+),(\d+): (.*)$/;

/**
 * The first place `fsPath` matched in a Copy All dump, if it is in there.
 *
 * Only the first: a search result is a single match, and which one the reader
 * clicked is not recoverable. The first is where the search view would have
 * taken someone who opened the file and pressed Find Next.
 */
export function firstSearchMatch(
  results: string,
  fsPath: string,
): SearchMatch | undefined {
  let inFile = false;
  for (const line of results.split(/\r?\n/)) {
    if (line === '') continue;
    const match = MATCH.exec(line);
    if (match) {
      if (!inFile) continue;
      return {
        line: Number(match[1]) - 1,
        column: Number(match[2]) - 1,
        text: match[3] ?? '',
      };
    }
    // A match that spans lines continues under its own first line, indented
    // like it; only an unindented line starts a new file.
    if (line.startsWith(' ')) continue;
    inFile = labels(line, fsPath);
  }
  return undefined;
}

/**
 * Whether a search-view path label names `fsPath`. The label is relative to the
 * workspace folder it was found in (or tildified, for a file outside one), so
 * it can only be matched as a trailing part of the real path.
 */
function labels(label: string, fsPath: string): boolean {
  const name = normalize(label);
  const target = normalize(fsPath);
  return name !== '' && (target === name || target.endsWith(`/${name}`));
}

/**
 * Compared case-insensitively throughout: the alternative is guessing at the
 * case sensitivity of a filesystem we are only holding a label for, and two
 * files that differ by case alone would still have to survive the caller's
 * check that the line reads as the search said it did.
 */
function normalize(value: string): string {
  return value
    .replace(/\\/g, '/')
    .replace(/^~\//, '')
    .replace(/^\.\//, '')
    .replace(/^\/+/, '')
    .toLowerCase();
}
