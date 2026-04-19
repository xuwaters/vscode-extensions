import * as vscode from 'vscode';
import { appendGitignore, buildGitignore } from './generate.js';
import { getTemplate, TEMPLATES, type Template } from './templates.js';

const DEFAULT_TEMPLATE_ID = 'default';

async function pickTemplates(): Promise<Template[] | undefined> {
  const items = TEMPLATES.map(t => ({
    label: t.label,
    description: t.description,
    id: t.id,
    picked: t.id === DEFAULT_TEMPLATE_ID,
  }));
  const picked = await vscode.window.showQuickPick(items, {
    canPickMany: true,
    placeHolder: 'Select templates to include in .gitignore',
    matchOnDescription: true,
  });
  if (!picked || picked.length === 0) return undefined;
  return picked.map(p => getTemplate(p.id)!).filter(Boolean);
}

function getWorkspaceFolder(): vscode.WorkspaceFolder | undefined {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    vscode.window.showErrorMessage('Gitignore Generator: Open a folder or workspace first.');
    return undefined;
  }
  if (folders.length === 1) return folders[0];
  return undefined;
}

async function chooseWorkspaceFolder(): Promise<vscode.WorkspaceFolder | undefined> {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    vscode.window.showErrorMessage('Gitignore Generator: Open a folder or workspace first.');
    return undefined;
  }
  if (folders.length === 1) return folders[0];
  return vscode.window.showWorkspaceFolderPick({ placeHolder: 'Target workspace folder for .gitignore' });
}

async function readFileIfExists(uri: vscode.Uri): Promise<string | undefined> {
  try {
    const bytes = await vscode.workspace.fs.readFile(uri);
    return new TextDecoder('utf-8').decode(bytes);
  } catch {
    return undefined;
  }
}

async function writeFile(uri: vscode.Uri, content: string): Promise<void> {
  const bytes = new TextEncoder().encode(content);
  await vscode.workspace.fs.writeFile(uri, bytes);
}

async function openDocument(uri: vscode.Uri): Promise<void> {
  const doc = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(doc);
}

export function activate(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.commands.registerCommand('gitignore-generator.generate', async () => {
      const folder = await chooseWorkspaceFolder();
      if (!folder) return;
      const templates = await pickTemplates();
      if (!templates) return;

      const uri = vscode.Uri.joinPath(folder.uri, '.gitignore');
      const existing = await readFileIfExists(uri);
      if (existing !== undefined) {
        const choice = await vscode.window.showWarningMessage(
          '.gitignore already exists. What would you like to do?',
          { modal: true },
          'Overwrite',
          'Append',
        );
        if (choice === 'Overwrite') {
          await writeFile(uri, buildGitignore(templates));
        } else if (choice === 'Append') {
          await writeFile(uri, appendGitignore(existing, templates));
        } else {
          return;
        }
      } else {
        await writeFile(uri, buildGitignore(templates));
      }
      await openDocument(uri);
    }),

    vscode.commands.registerCommand('gitignore-generator.append', async () => {
      const folder = getWorkspaceFolder() ?? (await chooseWorkspaceFolder());
      if (!folder) return;
      const templates = await pickTemplates();
      if (!templates) return;

      const uri = vscode.Uri.joinPath(folder.uri, '.gitignore');
      const existing = (await readFileIfExists(uri)) ?? '';
      await writeFile(uri, appendGitignore(existing, templates));
      await openDocument(uri);
    }),
  );
}

export function deactivate(): void {}
