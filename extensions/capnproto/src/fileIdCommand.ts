import * as vscode from 'vscode';
import { isCapnpDocument } from './diagnostics';
import { fileIdInsertion, findFileId, generateFileId } from './fileId';

export const GENERATE_FILE_ID_COMMAND = 'capnp.generateFileId';
const MISSING_FILE_ID_CODE = 'CAPNP0002';

/** The edit that gives `doc` a new file id, inserted or replacing the old one. */
function fileIdEdit(doc: vscode.TextDocument, id: string): vscode.WorkspaceEdit {
  const text = doc.getText();
  const edit = new vscode.WorkspaceEdit();
  const existing = findFileId(text);
  if (existing) {
    const range = new vscode.Range(doc.positionAt(existing.start), doc.positionAt(existing.end));
    edit.replace(doc.uri, range, id);
  } else {
    const { offset, text: insert } = fileIdInsertion(text, id);
    edit.insert(doc.uri, doc.positionAt(offset), insert);
  }
  return edit;
}

/**
 * Gives the active schema a file id. An existing id is only replaced after
 * confirmation, since that changes the id of every type in the file. Outside
 * a Cap'n Proto editor the new id goes to the clipboard, like `capnp id`.
 */
async function generateFileIdCommand(): Promise<void> {
  const id = generateFileId();
  const editor = vscode.window.activeTextEditor;
  if (!editor || !isCapnpDocument(editor.document.languageId)) {
    await vscode.env.clipboard.writeText(`${id};`);
    vscode.window.showInformationMessage(`Copied new Cap'n Proto file id ${id} to the clipboard.`);
    return;
  }

  const doc = editor.document;
  const existing = findFileId(doc.getText());
  if (existing) {
    const replace = 'Replace';
    const choice = await vscode.window.showWarningMessage(
      `This file already has the id ${existing.id}. Replace it with ${id}?`,
      {
        modal: true,
        detail:
          'Every type in the file derives its id from the file id, so existing ' +
          'serialized data and generated code will no longer match.',
      },
      replace,
    );
    if (choice !== replace) return;
  }

  await vscode.workspace.applyEdit(fileIdEdit(doc, id));
  const found = findFileId(doc.getText());
  if (found) {
    const pos = doc.positionAt(found.start);
    editor.revealRange(new vscode.Range(pos, pos));
  }
}

/** Quick fix on the "missing file id" diagnostic. */
class FileIdCodeActionProvider implements vscode.CodeActionProvider {
  static readonly kinds = [vscode.CodeActionKind.QuickFix];

  provideCodeActions(
    doc: vscode.TextDocument,
    _range: vscode.Range,
    context: vscode.CodeActionContext,
  ): vscode.CodeAction[] {
    const diag = context.diagnostics.find(
      (d) => d.source === 'capnp' && d.code === MISSING_FILE_ID_CODE,
    );
    if (!diag || findFileId(doc.getText())) return [];
    const action = new vscode.CodeAction('Generate file id', vscode.CodeActionKind.QuickFix);
    action.edit = fileIdEdit(doc, generateFileId());
    action.diagnostics = [diag];
    action.isPreferred = true;
    return [action];
  }
}

export function registerFileId(selector: vscode.DocumentSelector): vscode.Disposable[] {
  return [
    vscode.commands.registerCommand(GENERATE_FILE_ID_COMMAND, generateFileIdCommand),
    vscode.languages.registerCodeActionsProvider(selector, new FileIdCodeActionProvider(), {
      providedCodeActionKinds: FileIdCodeActionProvider.kinds,
    }),
  ];
}
