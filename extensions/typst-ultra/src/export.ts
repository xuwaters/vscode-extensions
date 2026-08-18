import * as path from 'path';
import * as vscode from 'vscode';
import type { Client } from './client.js';
import type { RootState } from './compileRoot.js';
import * as config from './config.js';
import { baseFor, expand } from './exportPath.js';
import { compileNow } from './preview/rpc.js';

/** What `typst/export` produces. */
interface ExportResult {
  /** Base64 payloads, one per output file, in page order. */
  files: string[];
  /** The extension to write them with. */
  extension: string;
}

/** The formats the server can produce. */
export type Format = 'pdf' | 'svg' | 'png' | 'html';

/** What each format is called, and what it is written as. */
const FORMATS: Record<
  Format,
  { label: string; extension: string; description: string }
> = {
  pdf: { label: 'PDF', extension: 'pdf', description: 'The whole document' },
  svg: { label: 'SVG', extension: 'svg', description: 'One file, all pages' },
  png: {
    label: 'PNG',
    extension: 'png',
    description: 'One file per page, 144 ppi',
  },
  html: {
    label: 'HTML',
    extension: 'html',
    description: 'A separate compilation target — experimental upstream',
  },
};

/**
 * Export a document.
 *
 * The server exports from its **last good document** and never compiles on
 * demand, so a document that currently has errors reports that rather than
 * quietly writing a stale file that looks current.
 */
export async function exportDocument(
  client: Client,
  root: RootState,
  format: Format,
  document: vscode.TextDocument,
): Promise<void> {
  // Ask where it goes *before* doing the work: a reader who changes their mind
  // at the dialog should not have paid for an export first.
  const base = await resolveTarget(format, document);
  if (!base) return;

  // Export writes the server's last good document, which is whichever file it
  // was last told to compile. Naming this one first is what stops "export" in a
  // two-document workspace writing the other one's pages under this one's name.
  // Notifications are delivered in order, and the compile is synchronous, so by
  // the time the request below is answered the subject has changed.
  compileNow(client, document.uri, root.pinned !== undefined);

  const result = await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Window,
      title: `Typst: exporting ${format}`,
    },
    async () =>
      client.request<ExportResult>('typst/export', {
        format,
        ppi: format === 'png' ? 144 : undefined,
      }),
  );

  if (!result) {
    void vscode.window.showErrorMessage(
      format === 'html'
        ? 'Typst: HTML export failed. It is a separate compilation target, so a document written for print may not produce one — see the log for the compiler\'s reason.'
        : 'Typst: cannot export — the document has not compiled successfully. Fix the errors and try again.',
    );
    return;
  }

  const written = await writeFiles(baseFor(base, result.extension), result);
  if (written.length === 0) return;

  const choice = await vscode.window.showInformationMessage(
    written.length === 1
      ? `Exported ${path.basename(written[0].fsPath)}`
      : `Exported ${written.length} files to ${path.dirname(written[0].fsPath)}`,
    'Open',
    'Reveal in Explorer',
  );

  if (choice === 'Open') {
    await vscode.env.openExternal(written[0]);
  } else if (choice === 'Reveal in Explorer') {
    await vscode.commands.executeCommand('revealFileInOS', written[0]);
  }
}

/** Ask which format, then export. */
export async function pickAndExport(
  client: Client,
  root: RootState,
  document: vscode.TextDocument,
): Promise<void> {
  const choice = await vscode.window.showQuickPick(
    (Object.keys(FORMATS) as Format[]).map((format) => ({
      label: FORMATS[format].label,
      description: FORMATS[format].description,
      format,
    })),
    { title: 'Typst Ultra: export', placeHolder: 'Which format?' },
  );
  if (choice) await exportDocument(client, root, choice.format, document);
}

/**
 * Where the export goes, as a path without its extension.
 *
 * The default is `typstUltra.export.outputPath`, which starts at `$dir/$name` —
 * the document's own folder, under the document's own name. The save dialog
 * opens *there*, so accepting it is one keystroke and moving it is a normal
 * file dialog rather than a settings trip. `export.askForLocation: false` skips
 * the dialog for anyone who has configured the path they want and would rather
 * not confirm it every time.
 */
async function resolveTarget(
  format: Format,
  document: vscode.TextDocument,
): Promise<string | undefined> {
  const settings = config.read(document.uri);
  const root = config.resolveRoot(settings, document.uri);
  const base = expand(settings.host.export.outputPath, document.uri.fsPath, root);
  const { label, extension } = FORMATS[format];

  if (!settings.host.export.askForLocation) return base;

  const chosen = await vscode.window.showSaveDialog({
    defaultUri: vscode.Uri.file(`${base}.${extension}`),
    filters: { [label]: [extension] },
    title: `Typst Ultra: export ${label}`,
    saveLabel: 'Export',
  });
  return chosen?.fsPath;
}

async function writeFiles(
  base: string,
  result: ExportResult,
): Promise<vscode.Uri[]> {
  const written: vscode.Uri[] = [];
  for (const [index, payload] of result.files.entries()) {
    const suffix = result.files.length > 1 ? `-${index + 1}` : '';
    const target = vscode.Uri.file(`${base}${suffix}.${result.extension}`);

    try {
      await vscode.workspace.fs.writeFile(target, decodeBase64(payload));
      written.push(target);
    } catch (error) {
      void vscode.window.showErrorMessage(
        `Typst: could not write ${target.fsPath}: ${String(error)}`,
      );
      return written;
    }
  }
  return written;
}

function decodeBase64(text: string): Uint8Array {
  return new Uint8Array(Buffer.from(text, 'base64'));
}
