import * as vscode from 'vscode';
import { Client } from './client.js';
import { CompileRoot } from './compileRoot.js';
import * as config from './config.js';
import { exportDocument, pickAndExport } from './export.js';
import { PreviewManager } from './preview/manager.js';
import { TypstPreviewEditor } from './preview/customEditor.js';
import { StatusBar } from './status.js';
import { createFromTemplate } from './template.js';

/**
 * Typst Ultra.
 *
 * Activation is deliberately cheap: `onLanguage:typst` fires, this runs, and
 * nothing heavy happens until a `.typ` is actually opened — at which point the
 * server process forks and instantiates the engine.
 */
export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel('Typst Ultra');
  const client = new Client(context, output);
  const preview = new PreviewManager(context, client, output);
  const compileRoot = new CompileRoot(context, client);
  const status = new StatusBar(client, output);

  context.subscriptions.push(output, client, preview, compileRoot, status);

  // Start on the first typst document, and on every later one in case the
  // server was stopped in between.
  const startForDocument = (document: vscode.TextDocument) => {
    if (document.languageId === 'typst') void client.start(document.uri);
  };
  vscode.workspace.textDocuments.forEach(startForDocument);
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(startForDocument),
  );

  // Tell the server which files exist, so `workspace/symbol` has something to
  // search. The host walks the file system; the server does not.
  const publishWorkspaceFiles = async () => {
    const files = await vscode.workspace.findFiles(
      '**/*.{typ,typc}',
      '**/node_modules/**',
      2000,
    );
    client.notify('typst/workspaceFiles', {
      uris: files.map((uri) => uri.toString()),
    });
  };
  void publishWorkspaceFiles();

  const watcher = vscode.workspace.createFileSystemWatcher('**/*.{typ,typc}');
  context.subscriptions.push(
    watcher,
    watcher.onDidCreate(() => void publishWorkspaceFiles()),
    watcher.onDidDelete(() => void publishWorkspaceFiles()),
  );

  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(async (event) => {
      if (!event.affectsConfiguration('typstUltra')) return;

      if (config.needsRestart(event)) {
        output.appendLine('configuration changed — restarting the server');
        await client.restart();
      } else {
        client.notify('workspace/didChangeConfiguration', {
          settings: { typstUltra: config.read().server },
        });
      }
      compileRoot.refresh();
    }),
  );

  context.subscriptions.push(
    TypstPreviewEditor.register(context, client, output),

    vscode.window.registerWebviewPanelSerializer('typstUltra.preview', {
      // The panel survives a window reload; pages are re-requested from
      // scratch with an empty `knownHashes`.
      async deserializeWebviewPanel(panel: vscode.WebviewPanel) {
        preview.adopt(panel);
        const active = vscode.window.activeTextEditor?.document;
        if (active?.languageId === 'typst') {
          await client.start(active.uri);
          preview.retarget(active.uri);
        }
      },
    }),

    vscode.commands.registerCommand('typstUltra.showPreview', async () => {
      const document = await requireTypstDocument();
      if (!document) return;
      await client.start(document.uri);
      await preview.show(document.uri, vscode.ViewColumn.Active);
    }),

    vscode.commands.registerCommand('typstUltra.showPreviewToSide', async () => {
      const document = await requireTypstDocument();
      if (!document) return;
      await client.start(document.uri);
      await preview.show(document.uri, vscode.ViewColumn.Beside);
    }),

    vscode.commands.registerCommand('typstUltra.syncPreviewToCursor', () =>
      preview.syncToCursor(),
    ),

    vscode.commands.registerCommand('typstUltra.toggleInvertColors', () =>
      preview.toggleInvert(),
    ),

    vscode.commands.registerCommand('typstUltra.pinMain', async () => {
      const document = await requireTypstDocument();
      if (document) await compileRoot.pin(document.uri);
    }),

    vscode.commands.registerCommand('typstUltra.unpinMain', () =>
      compileRoot.unpin(),
    ),

    vscode.commands.registerCommand('typstUltra.export', async () => {
      const document = await requireTypstDocument();
      if (document) await pickAndExport(client, document);
    }),

    vscode.commands.registerCommand('typstUltra.exportPdf', async () => {
      const document = await requireTypstDocument();
      if (document) await exportDocument(client, 'pdf', document);
    }),

    vscode.commands.registerCommand('typstUltra.restartServer', async () => {
      output.appendLine('restarting the language server');
      await client.restart();
    }),

    vscode.commands.registerCommand('typstUltra.showLog', () => output.show(true)),

    vscode.commands.registerCommand('typstUltra.newFromTemplate', async () => {
      // Templates come from the package registry, so the server has to be up
      // even if no `.typ` is open yet.
      await client.start();
      await createFromTemplate(client);
    }),

    vscode.commands.registerCommand('typstUltra.clearPackageCache', async () => {
      const choice = await vscode.window.showWarningMessage(
        'Delete the Typst package cache? Packages will be downloaded again on the next compile.',
        { modal: true },
        'Delete',
      );
      if (choice !== 'Delete') return;
      client.notify('typst/clearPackageCache', {});
      void vscode.window.showInformationMessage('Typst: package cache cleared.');
    }),
  );
}

export function deactivate(): void {
  // Everything is in `context.subscriptions`.
}

/** The active typst document, complaining clearly if there is not one. */
async function requireTypstDocument(): Promise<vscode.TextDocument | undefined> {
  const editor = vscode.window.activeTextEditor;
  if (editor?.document.languageId === 'typst') return editor.document;

  const open = vscode.workspace.textDocuments.find(
    (document) => document.languageId === 'typst',
  );
  if (open) return open;

  void vscode.window.showInformationMessage(
    'Typst: open a .typ file first.',
  );
  return undefined;
}
