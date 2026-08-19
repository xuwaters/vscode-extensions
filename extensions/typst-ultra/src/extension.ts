import * as vscode from 'vscode';
import { exportDocument, pickAndExport } from './commands/export.js';
import { namesTypstSource } from './commands/target.js';
import { createFromTemplate } from './commands/template.js';
import { CompileRoot } from './compileRoot.js';
import * as config from './config.js';
import { Client } from './lsp/client.js';
import { StatusBar } from './lsp/status.js';
import { TypstPreviewEditor } from './preview/customEditor.js';
import { PreviewManager } from './preview/manager.js';
import { ModeManager } from './preview/modes.js';
import { PageMemory } from './preview/pageMemory.js';

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
  // The compile root outranks the preview's own idea of what to show, so it is
  // built first and handed to everything that has to respect a pinned file.
  const compileRoot = new CompileRoot(context, client);
  // Where each document was last read to, shared by both preview surfaces so a
  // mode switch between them is continuous.
  const pages = new PageMemory();
  const preview = new PreviewManager(context, client, output, pages, compileRoot);
  const modes = new ModeManager(preview);
  const status = new StatusBar(client, output);

  context.subscriptions.push(output, client, preview, modes, compileRoot, status);

  // Tell the server which files exist, so `workspace/symbol` and citation
  // completion have something to search. The host walks the file system; the
  // server does not. `.bib` files are in the list because a bibliography the
  // compile has not read yet is still a bibliography the reader can cite from.
  const publishWorkspaceFiles = async () => {
    const files = await vscode.workspace.findFiles(
      '**/*.{typ,typc,bib}',
      '**/node_modules/**',
      2000,
    );
    client.notify('typst/workspaceFiles', {
      uris: files.map((uri) => uri.toString()),
    });
  };

  /** Whether this workspace is a typst project at all, looked up once. */
  let typstProject: Promise<boolean> | undefined;
  const isTypstProject = async (): Promise<boolean> => {
    typstProject ??= (async () => {
      const found = await vscode.workspace.findFiles(
        '**/*.{typ,typc}',
        '**/node_modules/**',
        1,
      );
      return found.length > 0;
    })();
    return typstProject;
  };

  // Start on the first typst document, and on every later one in case the
  // server was stopped in between.
  const startForDocument = async (document: vscode.TextDocument) => {
    if (document.languageId === 'typst') {
      // fall through
    } else if (document.languageId === 'bibtex') {
      // A `.bib` on its own is not reason enough to fork a WASM engine —
      // someone editing a LaTeX project's bibliography should never notice this
      // extension. It starts once the workspace turns out to hold typst files.
      if (!client.running && !(await isTypstProject())) return;
    } else {
      return;
    }

    const wasRunning = client.running;
    await client.start(document.uri);
    // The file list is only useful once there is a server to receive it, and
    // activation runs before the first document opens.
    if (!wasRunning) void publishWorkspaceFiles();
  };
  vscode.workspace.textDocuments.forEach((document) => void startForDocument(document));
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(
      (document) => void startForDocument(document),
    ),
  );

  const watcher = vscode.workspace.createFileSystemWatcher('**/*.{typ,typc,bib}');
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

  /**
   * The file a command's argument names, if it names one at all — see
   * `namesTypstSource` for who passes what. A code lens sends its URI as a
   * string, because the language server's arguments are JSON.
   */
  const namedSource = (target?: vscode.Uri | string): vscode.Uri | undefined => {
    if (!namesTypstSource(target)) return undefined;
    if (typeof target !== 'string') return target;
    try {
      return vscode.Uri.parse(target, true);
    } catch {
      return undefined;
    }
  };

  /**
   * Which document a command acts on. An argument that names a file wins;
   * anything else — no argument, a webview's own resource, a URI that will not
   * parse — means the document the reader is looking at.
   */
  const requireDocument = async (
    target?: vscode.Uri | string,
  ): Promise<vscode.TextDocument | undefined> => {
    const named = namedSource(target);
    if (named) return vscode.workspace.openTextDocument(named);

    const editor = vscode.window.activeTextEditor;
    if (editor?.document.languageId === 'typst') return editor.document;

    // The focus may be inside the preview, which is no editor at all.
    const shown = preview.currentUri;
    if (shown) return vscode.workspace.openTextDocument(shown);

    const open = vscode.workspace.textDocuments.find(
      (document) => document.languageId === 'typst',
    );
    if (open) return open;

    void vscode.window.showInformationMessage('Typst: open a .typ file first.');
    return undefined;
  };

  context.subscriptions.push(
    TypstPreviewEditor.register(context, client, output, pages, compileRoot),

    vscode.window.registerWebviewPanelSerializer(PreviewManager.viewType, {
      // The panel survives a window reload; the manager works out what it was
      // showing, and pages are re-requested from scratch with an empty
      // `knownHashes`.
      deserializeWebviewPanel(panel: vscode.WebviewPanel) {
        return preview.restore(panel);
      },
    }),

    vscode.commands.registerCommand(
      'typstUltra.showPreview',
      async (target?: vscode.Uri | string) => {
        const document = await requireDocument(target);
        if (document) await preview.show(document.uri, vscode.ViewColumn.Active);
      },
    ),

    vscode.commands.registerCommand(
      'typstUltra.showPreviewToSide',
      async (target?: vscode.Uri | string) => {
        const document = await requireDocument(target);
        if (document) await preview.show(document.uri, vscode.ViewColumn.Beside);
      },
    ),

    vscode.commands.registerCommand('typstUltra.cycleMode', () =>
      modes.cycleMode(),
    ),
    vscode.commands.registerCommand('typstUltra.switchMode', () =>
      modes.switchMode(),
    ),
    vscode.commands.registerCommand('typstUltra.toggleEditPreview', () =>
      modes.toggleEditPreview(),
    ),
    vscode.commands.registerCommand('typstUltra.setModeEdit', () =>
      modes.setMode('edit'),
    ),
    vscode.commands.registerCommand('typstUltra.setModeSplit', () =>
      modes.setMode('split'),
    ),
    vscode.commands.registerCommand('typstUltra.setModePreview', () =>
      modes.setMode('preview'),
    ),

    vscode.commands.registerCommand('typstUltra.toggleFocus', () =>
      preview.toggleFocus(),
    ),

    vscode.commands.registerCommand('typstUltra.togglePreviewLock', () => {
      if (!preview.hasPreview) {
        void vscode.window.showInformationMessage('Typst: no preview is open to pin.');
        return;
      }
      const locked = preview.toggleLock();
      void vscode.window.showInformationMessage(
        locked
          ? 'Typst: the preview is pinned to this file.'
          : 'Typst: the preview will follow the active editor.',
      );
    }),

    vscode.commands.registerCommand('typstUltra.syncPreviewToCursor', () =>
      preview.syncToCursor(),
    ),

    vscode.commands.registerCommand('typstUltra.toggleInvertColors', () =>
      preview.toggleInvert(),
    ),

    vscode.commands.registerCommand('typstUltra.pinMain', async () => {
      const document = await requireDocument();
      if (document) await compileRoot.pin(document.uri);
    }),

    vscode.commands.registerCommand('typstUltra.unpinMain', () =>
      compileRoot.unpin(),
    ),

    vscode.commands.registerCommand(
      'typstUltra.export',
      async (target?: vscode.Uri | string) => {
        const document = await requireDocument(target);
        if (document) await pickAndExport(client, compileRoot, document);
      },
    ),

    vscode.commands.registerCommand(
      'typstUltra.exportPdf',
      async (target?: vscode.Uri | string) => {
        const document = await requireDocument(target);
        if (document) {
          await exportDocument(client, compileRoot, 'pdf', document);
        }
      },
    ),

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
