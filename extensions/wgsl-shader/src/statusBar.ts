// The GLSL stage indicator.
//
// GLSL carries no record of its own stage, and naga needs one before it can
// parse at all. Which stage was picked — and whether the file was validated
// under it — is otherwise invisible, and a file silently checked as the wrong
// stage produces errors that make no sense.
//
// The answer comes from the server, over the one non-standard request in the
// protocol: only the server knows what it actually did.

import * as vscode from 'vscode';

import type { Client } from './lsp/client.js';

/** The reply to `wgsl/shaderInfo`. */
export interface ShaderInfo {
  language: 'wgsl' | 'glsl';
  stage?: string;
  skipped?: string;
  ok: boolean;
  problems: number;
  naga: string;
}

const UNSUPPORTED = 'unsupported';

export function register(context: vscode.ExtensionContext, client: Client): void {
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  status.command = 'glsl.showShaderStage';
  context.subscriptions.push(status);

  async function update(editor: vscode.TextEditor | undefined): Promise<void> {
    const document = editor?.document;
    if (!document || document.languageId !== 'glsl') {
      status.hide();
      return;
    }
    const enabled = vscode.workspace
      .getConfiguration('glsl', document.uri)
      .get<boolean>('showStageInStatusBar', true);
    if (!enabled) {
      status.hide();
      return;
    }

    const info = await shaderInfo(client, document.uri);
    if (!info?.stage) {
      status.hide();
      return;
    }

    const label = info.stage === UNSUPPORTED ? 'stage unknown' : info.stage;
    status.text = info.skipped ? `GLSL: ${label} (not validated)` : `GLSL: ${label}`;
    status.tooltip = info.skipped
      ? `${info.skipped}. This file is highlighted and analysed, but not validated.`
      : `Validated as a ${info.stage} shader by naga ${info.naga}. ` +
        'Add #pragma shader_stage(…) to override.';
    status.show();
  }

  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor((editor) => void update(editor)),
    vscode.workspace.onDidSaveTextDocument(
      () => void update(vscode.window.activeTextEditor),
    ),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration('glsl.showStageInStatusBar')) {
        void update(vscode.window.activeTextEditor);
      }
    }),
  );

  void update(vscode.window.activeTextEditor);
}

export async function shaderInfo(
  client: Client,
  uri: vscode.Uri,
): Promise<ShaderInfo | undefined> {
  return client.request<ShaderInfo>('wgsl/shaderInfo', {
    textDocument: { uri: uri.toString() },
  });
}
