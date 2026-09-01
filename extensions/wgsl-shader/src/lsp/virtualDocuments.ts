// Surfacing an embedded shader to the language server, and its answers back.
//
// Each `/* wgsl */ "…"` block in a host file becomes a virtual document under
// the `wgsl-embedded:` scheme, holding the host file with everything outside
// that block blanked out (see `embedded.ts`). Because the blanking preserves
// length and line structure, a `Position` means the same thing in both
// documents — so forwarding a request is `executeCommand(…, virtualUri,
// position)` with no translation at all, and the answers come back already in
// the host's coordinates.
//
// The virtual document's path ends in `.wgsl` or `.glsl`, which is how VS Code
// gives it the right language id, which is how the client's document selector
// picks it up, which is how the server ever sees it.

import * as vscode from 'vscode';

import {
  findEmbeddedBlocks,
  hostLanguage,
  virtualDocument,
  type EmbeddedBlock,
  type ShaderLanguage,
} from './embedded.js';

export const EMBEDDED_SCHEME = 'wgsl-embedded';

/** The host languages whose string literals we look inside. */
export const HOST_LANGUAGES = [
  'rust',
  'typescript',
  'typescriptreact',
  'javascript',
  'javascriptreact',
] as const;

/** The virtual document for one block of a host file. */
export function virtualUri(
  host: vscode.Uri,
  index: number,
  language: ShaderLanguage,
): vscode.Uri {
  return vscode.Uri.from({
    scheme: EMBEDDED_SCHEME,
    authority: 'shader',
    // The extension is load-bearing: VS Code reads the language id off it.
    path: `/${index}.${language}`,
    query: host.toString(),
  });
}

/** The host file a virtual document was extracted from. */
export function hostOf(uri: vscode.Uri): vscode.Uri {
  return vscode.Uri.parse(uri.query);
}

function blockIndexOf(uri: vscode.Uri): number {
  const name = uri.path.replace(/^\//, '').split('.')[0] ?? '';
  return Number.parseInt(name, 10);
}

/** The blocks in a document, or none if it is not a host language. */
export function blocksIn(document: vscode.TextDocument): EmbeddedBlock[] {
  const host = hostLanguage(document.languageId);
  if (!host) return [];
  return findEmbeddedBlocks(document.getText(), host);
}

/** Wire up the virtual documents and the providers that forward onto them. */
export function register(context: vscode.ExtensionContext, onOpen: () => void): void {
  const changed = new vscode.EventEmitter<vscode.Uri>();
  /** Virtual documents that have been handed out, so changes can reach them. */
  const live = new Map<string, vscode.Uri>();

  const provider: vscode.TextDocumentContentProvider = {
    onDidChange: changed.event,
    provideTextDocumentContent(uri) {
      const host = hostOf(uri);
      const document = vscode.workspace.textDocuments.find(
        (candidate) => candidate.uri.toString() === host.toString(),
      );
      if (!document) return '';

      const block = blocksIn(document)[blockIndexOf(uri)];
      return block ? virtualDocument(document.getText(), block) : '';
    },
  };

  context.subscriptions.push(
    changed,
    vscode.workspace.registerTextDocumentContentProvider(EMBEDDED_SCHEME, provider),
    // Every edit to a host file invalidates its blocks. Firing for all of them
    // is cheap — there are rarely more than a handful — and firing for the
    // wrong subset is a stale completion list.
    vscode.workspace.onDidChangeTextDocument((event) => {
      const host = event.document.uri.toString();
      for (const uri of live.values()) {
        if (uri.query === host) changed.fire(uri);
      }
    }),
    vscode.workspace.onDidCloseTextDocument((document) => {
      if (document.uri.scheme === EMBEDDED_SCHEME) live.delete(document.uri.toString());
    }),
  );

  /**
   * The virtual document for the block at `position`, opened and ready to be
   * asked. `undefined` when the cursor is not inside a shader.
   */
  async function target(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): Promise<vscode.Uri | undefined> {
    if (!enabled(document)) return undefined;

    const offset = document.offsetAt(position);
    const blocks = blocksIn(document);
    const index = blocks.findIndex(
      (block) => offset >= block.start && offset <= block.end,
    );
    if (index === -1) return undefined;

    // Starting the server here rather than at activation is what keeps a Rust
    // project that embeds no shaders from paying for one.
    onOpen();

    const uri = virtualUri(document.uri, index, blocks[index]!.language);
    live.set(uri.toString(), uri);
    await vscode.workspace.openTextDocument(uri);
    return uri;
  }

  const selector = HOST_LANGUAGES.map((language) => ({ language }));

  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      selector,
      {
        async provideCompletionItems(document, position, _token, context) {
          const uri = await target(document, position);
          if (!uri) return undefined;
          return vscode.commands.executeCommand<vscode.CompletionList>(
            'vscode.executeCompletionItemProvider',
            uri,
            position,
            context.triggerCharacter,
          );
        },
      },
      '.',
      '@',
      '#',
      '<',
    ),

    vscode.languages.registerHoverProvider(selector, {
      async provideHover(document, position) {
        const uri = await target(document, position);
        if (!uri) return undefined;
        const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
          'vscode.executeHoverProvider',
          uri,
          position,
        );
        return hovers?.[0];
      },
    }),

    vscode.languages.registerDefinitionProvider(selector, {
      async provideDefinition(document, position) {
        const uri = await target(document, position);
        if (!uri) return undefined;
        const locations = await vscode.commands.executeCommand<vscode.Location[]>(
          'vscode.executeDefinitionProvider',
          uri,
          position,
        );
        // Positions already line up; only the URI has to come back to the
        // file the user is actually looking at.
        return locations?.map(
          (location) =>
            new vscode.Location(
              location.uri.scheme === EMBEDDED_SCHEME ? document.uri : location.uri,
              location.range,
            ),
        );
      },
    }),

    vscode.languages.registerDocumentHighlightProvider(selector, {
      async provideDocumentHighlights(document, position) {
        const uri = await target(document, position);
        if (!uri) return undefined;
        return vscode.commands.executeCommand<vscode.DocumentHighlight[]>(
          'vscode.executeDocumentHighlights',
          uri,
          position,
        );
      },
    }),

    vscode.languages.registerSignatureHelpProvider(
      selector,
      {
        async provideSignatureHelp(document, position) {
          const uri = await target(document, position);
          if (!uri) return undefined;
          return vscode.commands.executeCommand<vscode.SignatureHelp>(
            'vscode.executeSignatureHelpProvider',
            uri,
            position,
          );
        },
      },
      '(',
      ',',
    ),
  );
}

/** Whether embedded support is switched on for a document's shader languages. */
function enabled(document: vscode.TextDocument): boolean {
  return (
    vscode.workspace
      .getConfiguration('wgsl', document.uri)
      .get<boolean>('embedded.enabled', true) ||
    vscode.workspace
      .getConfiguration('glsl', document.uri)
      .get<boolean>('embedded.enabled', true)
  );
}
