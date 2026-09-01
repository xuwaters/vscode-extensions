// Telling the server which shader files exist.
//
// The server runs inside WASM and has no filesystem, so `workspace/symbol` can
// only see the files the host hands it. This walks the workspace once after the
// server starts, then keeps the list current from a file watcher.
//
// Only the outline is kept on the far side, not the text, so the cost of a
// large workspace is a few hundred kilobytes of names.

import * as vscode from 'vscode';

import type { Client } from './lsp/client.js';

/** The extensions `package.json` registers for the two languages. */
const PATTERN =
  '**/*.{wgsl,glsl,vert,frag,comp,geom,tesc,tese,vsh,fsh,gsh,vshader,fshader,gshader,glslv,glslf,vertexshader,fragmentshader}';

const EXCLUDE = '**/{node_modules,target,dist,out,.git}/**';

/**
 * How many files to index.
 *
 * A cap rather than a promise to index everything: past a few thousand shaders
 * the symbol picker is not the tool anyone reaches for, and the cost is paid on
 * every window that opens the project.
 */
const LIMIT = 2000;

export function register(context: vscode.ExtensionContext, client: Client): void {
  let scheduled: NodeJS.Timeout | undefined;

  /**
   * Rescan and push. Debounced: a `git checkout` fires the watcher once per
   * file, and one walk afterwards answers all of them.
   */
  function schedule(delay = 300): void {
    if (scheduled) clearTimeout(scheduled);
    scheduled = setTimeout(() => {
      scheduled = undefined;
      void scan(client);
    }, delay);
  }

  const watcher = vscode.workspace.createFileSystemWatcher(PATTERN);
  context.subscriptions.push(
    watcher,
    watcher.onDidCreate(() => schedule()),
    watcher.onDidDelete(() => schedule()),
    // Content changes reach the server through `didChange` while a file is
    // open; the watcher matters for edits made outside the editor.
    watcher.onDidChange(() => schedule(1000)),
    vscode.workspace.onDidChangeWorkspaceFolders(() => schedule(0)),
    new vscode.Disposable(() => {
      if (scheduled) clearTimeout(scheduled);
    }),
  );

  schedule(0);
}

async function scan(client: Client): Promise<void> {
  if (!vscode.workspace.workspaceFolders?.length) return;

  const uris = await vscode.workspace.findFiles(PATTERN, EXCLUDE, LIMIT);
  const files: { uri: string; languageId: string; text: string }[] = [];

  for (const uri of uris) {
    const languageId = uri.path.endsWith('.wgsl') ? 'wgsl' : 'glsl';
    try {
      const bytes = await vscode.workspace.fs.readFile(uri);
      files.push({ uri: uri.toString(), languageId, text: new TextDecoder().decode(bytes) });
    } catch {
      // Deleted between the walk and the read, or unreadable. Skipping it is
      // the whole of the recovery: the next scan will settle it.
    }
  }

  client.notify('wgsl/workspaceFiles', { files, replace: true });
}
