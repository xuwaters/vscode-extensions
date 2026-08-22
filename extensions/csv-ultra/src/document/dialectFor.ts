import * as vscode from 'vscode';
import { readDelimiter } from '../config.js';
import { resolveDialect, type Dialect } from '../csv/dialect.js';

/**
 * How much of a document's head settles its dialect.
 *
 * The sniffer scores its candidates over the first few dozen records and the
 * line ending comes from the first break in the file, so nothing past this can
 * change the answer.
 */
const SNIFF_CHARS = 1 << 16;

/**
 * The dialect one document is read with.
 *
 * The slice is the point. This is asked on every committed cell, every scroll of
 * a coloured text editor and every command, and `document.getText()` with no
 * range is a copy of the whole file — thirty megabytes of string allocated to
 * look at the first sixty-four kilobytes of it. Three callers used to do that
 * three different ways; now there is one.
 *
 * @param override A delimiter chosen for one tab, outranking the configuration.
 */
export function dialectFor(document: vscode.TextDocument, override?: string): Dialect {
  return resolveDialect({
    configured: override ?? readDelimiter(document.uri),
    path: document.uri.path,
    text: document.getText(
      new vscode.Range(new vscode.Position(0, 0), document.positionAt(SNIFF_CHARS)),
    ),
  });
}
