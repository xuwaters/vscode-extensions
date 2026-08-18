/**
 * Whether the argument a command was invoked with names typst source.
 *
 * Commands here are invoked from four places, and no two of them agree on what
 * they hand over:
 *
 * * The explorer context menu passes the file's `Uri`.
 * * A code lens comes from the language server, where a command's arguments are
 *   plain JSON — so the same file arrives as the *string* `file:///…/paper.typ`.
 * * The palette and the keybindings pass nothing, meaning the active editor.
 * * A tab's own toolbar passes that tab's resource, which for the preview panel
 *   is the webview rather than a file:
 *   `webview-panel:webview-panel/webview-typstUltra.preview-…`.
 *
 * The last two both mean "work it out", and an argument that names no source
 * file has to be treated as absent rather than opened: `openTextDocument` reads
 * a string as a *file path*, so the code lens's URI sent it looking for a file
 * literally named `file:///…`, and the webview's URI resolves to nothing at all.
 *
 * The test is the file name rather than a list of rejected schemes, because a
 * caller cannot enumerate every synthetic editor resource VSCode may invent. An
 * untitled buffer is not named `.typ` and so takes the "work it out" path,
 * arriving at the active editor — which is the document it was already about.
 */
export function namesTypstSource(target: unknown): boolean {
  const path = typeof target === 'string' ? target : pathOf(target);
  return typeof path === 'string' && /\.typc?$/i.test(path);
}

function pathOf(target: unknown): unknown {
  return typeof target === 'object' && target !== null
    ? (target as { path?: unknown }).path
    : undefined;
}
