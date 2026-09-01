// Activation.
//
// Everything that used to live here — a word-list completion provider, a regex
// symbol provider, a diagnostic collection — is now in Rust, behind a real
// language server. What is left is the parts that are genuinely about VS Code:
// starting the client, the GLSL stage indicator, the embedded-shader
// documents, and the rust-analyzer hint.

import * as vscode from 'vscode';

import * as config from './config.js';
import { Client } from './lsp/client.js';
import * as virtualDocuments from './lsp/virtualDocuments.js';
import {
  HINT_ACTIONS,
  HINT_SETTING,
  RUST_ANALYZER_EXTENSION_ID,
  RUST_STRING_TOKENS_SETTING,
  hintActions,
  shouldOfferStringTokenFix,
} from './rustHint.js';
import * as statusBar from './statusBar.js';
import * as workspaceFiles from './workspaceFiles.js';

export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel('WGSL / GLSL Shader');
  const client = new Client(context, output);
  context.subscriptions.push(output, client);

  registerRustHighlightHint(context);
  statusBar.register(context, client);
  workspaceFiles.register(context, client);
  virtualDocuments.register(context, () => void client.start());

  context.subscriptions.push(
    client.onNotification('wgsl/engineMissing', (params) => {
      const { command } = params as { command: string };
      output.appendLine(`The shader engine is not built. Run \`${command}\`.`);
      void vscode.window.showErrorMessage(
        `The WGSL/GLSL engine is not built. Run \`${command}\` in extensions/wgsl-shader.`,
      );
    }),
  );

  registerCommands(context, client);

  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (!config.affectsServer(event)) return;
      // A client reads server capabilities once, so a setting that decides
      // whether a request is advertised at all needs the server back up.
      // Everything else is read per request and travels as a notification.
      if (config.needsRestart(event)) {
        void client.restart();
      } else {
        client.sendConfiguration();
      }
    }),
  );

  // A shader file that is already open is why we were activated.
  if (isShader(vscode.window.activeTextEditor?.document)) {
    void client.start();
  }
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((document) => {
      if (isShader(document)) void client.start();
    }),
  );
}

/** A type predicate, so a guarded `document` narrows for the caller. */
function isShader(
  document: vscode.TextDocument | undefined,
): document is vscode.TextDocument {
  return document?.languageId === 'wgsl' || document?.languageId === 'glsl';
}

function registerCommands(context: vscode.ExtensionContext, client: Client): void {
  const validate = async () => {
    const document = vscode.window.activeTextEditor?.document;
    if (!isShader(document)) return;
    await client.start();
    // Distinct from `didSave`, which honours the `validate.onSave` setting.
    // Asking for validation explicitly should validate.
    client.notify('wgsl/validate', { textDocument: { uri: document.uri.toString() } });

    // A clean file publishes nothing, so without this the command would look
    // like it had done nothing at all.
    const info = await statusBar.shaderInfo(client, document.uri);
    if (!info) return;
    if (info.skipped) {
      void vscode.window.showWarningMessage(`Not validated: ${info.skipped}.`);
    } else if (info.ok) {
      void vscode.window.showInformationMessage(
        `No problems found by naga ${info.naga}.`,
      );
    }
  };

  context.subscriptions.push(
    vscode.commands.registerCommand('wgsl.validateFile', validate),
    vscode.commands.registerCommand('glsl.validateFile', validate),
    vscode.commands.registerCommand('wgsl.restartServer', () => client.restart()),
    vscode.commands.registerCommand('glsl.showShaderStage', async () => {
      const document = vscode.window.activeTextEditor?.document;
      if (document?.languageId !== 'glsl') return;

      const info = await statusBar.shaderInfo(client, document.uri);
      if (!info) {
        void vscode.window.showInformationMessage('The shader server has no answer yet.');
        return;
      }
      const stage = info.stage ?? 'unknown';
      void vscode.window.showInformationMessage(
        info.skipped
          ? `Treated as a ${stage} shader, but not validated: ${info.skipped}.`
          : `Validated as a ${stage} shader by naga ${info.naga}. ` +
            `${info.problems === 0 ? 'No problems found.' : `${info.problems} problem(s) found.`}`,
      );
    }),
  );
}

/**
 * Offer, once per session, to turn off the rust-analyzer setting that hides the
 * shader highlighting inside tagged Rust strings. Asked only when a Rust file
 * actually uses the tag, so plain Rust users never see it.
 */
function registerRustHighlightHint(context: vscode.ExtensionContext): void {
  let asked = false;

  async function consider(document: vscode.TextDocument): Promise<void> {
    if (asked) return;

    const wgslConfig = vscode.workspace.getConfiguration('wgsl');
    const offer = shouldOfferStringTokenFix({
      languageId: document.languageId,
      text: document.getText(),
      hasRustAnalyzer: vscode.extensions.getExtension(RUST_ANALYZER_EXTENSION_ID) !== undefined,
      stringTokensEnabled: vscode.workspace
        .getConfiguration()
        .get<boolean>(RUST_STRING_TOKENS_SETTING, true),
      hintEnabled: wgslConfig.get<boolean>(HINT_SETTING, true),
    });
    if (!offer) return;

    asked = true;

    const hasWorkspace = (vscode.workspace.workspaceFolders?.length ?? 0) > 0;
    const choice = await vscode.window.showInformationMessage(
      'rust-analyzer highlights whole string literals, which hides the shader colouring in /* wgsl */ and /* glsl */ strings. ' +
        `Turn off ${RUST_STRING_TOKENS_SETTING}? Rust strings keep their colour from the TextMate grammar.`,
      ...hintActions(hasWorkspace),
    );

    if (choice === HINT_ACTIONS.workspace || choice === HINT_ACTIONS.global) {
      const target =
        choice === HINT_ACTIONS.workspace
          ? vscode.ConfigurationTarget.Workspace
          : vscode.ConfigurationTarget.Global;
      await vscode.workspace.getConfiguration().update(RUST_STRING_TOKENS_SETTING, false, target);
    } else if (choice === HINT_ACTIONS.never) {
      await wgslConfig.update(HINT_SETTING, false, vscode.ConfigurationTarget.Global);
    }
  }

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((document) => {
      void consider(document);
    }),
  );

  // The file that triggered activation is already open.
  const active = vscode.window.activeTextEditor?.document;
  if (active) void consider(active);
}

export function deactivate(): void {
  // Disposal of the client is handled through `context.subscriptions`.
}
