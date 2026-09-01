// The GLSL stage indicator.
//
// GLSL carries no record of its own stage, and the analyzer needs one before
// it can decide which builtins a file has. Which stage was picked — and
// whether it was *declared* or guessed — is otherwise invisible, and a file
// silently checked as the wrong stage produces errors that make no sense.
//
// The answer comes from the server, over the one non-standard request in the
// protocol: only the server knows what it actually did.

import * as vscode from 'vscode';

import type { Client } from './lsp/client.js';

/** The reply to `wgsl/shaderInfo`. */
export interface ShaderInfo {
  language: 'wgsl' | 'glsl';
  stage?: string;
  /** Whether the stage was guessed from the source rather than declared. */
  stageGuessed: boolean;
  /** The GLSL version in force, as the spec writes it: `4.50`, `3.00 es`. */
  version?: string;
  ok: boolean;
  problems: number;
  warnings: number;
  /** Who did the checking — `glsl-analysis 0.1.0`, `naga 30.0.1`. */
  validator: string;
  naga: string;
}

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

    // A guessed stage is worth flagging: it is the one input to the analysis
    // that nothing in the file states, and a wrong guess is a wrong answer.
    const version = info.version ? ` ${info.version}` : '';
    status.text = info.stageGuessed
      ? `GLSL${version}: ${info.stage}?`
      : `GLSL${version}: ${info.stage}`;
    status.tooltip = [
      `Analysed as a ${info.stage} shader`,
      info.version ? ` against GLSL ${info.version}` : '',
      ` by ${info.validator}.`,
      info.stageGuessed
        ? ' The stage was guessed from the built-ins this file uses — add' +
          ' #pragma shader_stage(…) to say for certain.'
        : ' Add #pragma shader_stage(…) to override.',
      info.version === undefined || info.stageGuessed
        ? ''
        : ' Set glsl.defaultVersion for files that declare no #version.',
    ].join('');
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
