import * as path from 'path';

/**
 * Expand `typstUltra.export.outputPath`.
 *
 * `$dir` is the document's directory, `$name` its stem, `$root` the compile
 * root. The result carries no extension; the format supplies that.
 *
 * Separate from `export.ts` because it is pure string handling with no business
 * knowing about VSCode — which also means it can be tested without stubbing the
 * whole editor API.
 */
export function expand(
  template: string,
  documentPath: string,
  root: string,
): string {
  const dir = path.dirname(documentPath);
  const name = path.basename(documentPath, path.extname(documentPath));

  const expanded = template
    .replace(/\$dir\b/g, dir)
    .replace(/\$name\b/g, name)
    .replace(/\$root\b/g, root);

  return path.isAbsolute(expanded) ? expanded : path.join(root, expanded);
}

/**
 * The base path an export writes to, given what the reader chose in the save
 * dialog.
 *
 * The dialog deals in whole file names and the writer deals in a base plus a
 * format's extension — PNG produces one file per page, so the base is the part
 * that has to survive. Only a trailing extension that *matches the format* is
 * removed: `paper.v2` keeps its `.v2`, because a reader who typed it meant it.
 * Case-insensitive, because `PAPER.PDF` came from a file picker on a file
 * system that does not care.
 */
export function baseFor(chosenPath: string, extension: string): string {
  const suffix = `.${extension}`;
  return chosenPath.toLowerCase().endsWith(suffix.toLowerCase())
    ? chosenPath.slice(0, -suffix.length)
    : chosenPath;
}
