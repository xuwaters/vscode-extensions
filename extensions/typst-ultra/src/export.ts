import * as path from 'path';
import * as vscode from 'vscode';
import type { Client } from './client.js';
import * as config from './config.js';
import { expand } from './exportPath.js';

/** What `typst/export` produces. */
interface ExportResult {
  /** Base64 payloads, one per output file, in page order. */
  files: string[];
  /** The extension to write them with. */
  extension: string;
}

/** The formats the server can produce. */
export type Format = 'pdf' | 'svg' | 'png' | 'html';

/**
 * Export a document.
 *
 * The server exports from its **last good document** and never compiles on
 * demand, so a document that currently has errors reports that rather than
 * quietly writing a stale file that looks current.
 */
export async function exportDocument(
  client: Client,
  format: Format,
  document: vscode.TextDocument,
): Promise<void> {
  const result = await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Window, title: `Typst: exporting ${format}` },
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

  const written = await writeFiles(document, result);
  if (written.length === 0) return;

  const choice = await vscode.window.showInformationMessage(
    written.length === 1
      ? `Exported ${path.basename(written[0].fsPath)}`
      : `Exported ${written.length} files`,
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
  document: vscode.TextDocument,
): Promise<void> {
  const choice = await vscode.window.showQuickPick(
    [
      { label: 'PDF', description: 'The whole document', format: 'pdf' as const },
      { label: 'SVG', description: 'One file, all pages', format: 'svg' as const },
      { label: 'PNG', description: 'One file per page, 144 ppi', format: 'png' as const },
      {
        label: 'HTML',
        description: 'A separate compilation target — experimental upstream',
        format: 'html' as const,
      },
    ],
    { title: 'Typst: export', placeHolder: 'Which format?' },
  );
  if (choice) await exportDocument(client, choice.format, document);
}

async function writeFiles(
  document: vscode.TextDocument,
  result: ExportResult,
): Promise<vscode.Uri[]> {
  const settings = config.read(document.uri);
  const root = config.resolveRoot(settings, document.uri);
  const base = expand(settings.host.export.outputPath, document.uri.fsPath, root);

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
