import * as path from 'path';
import * as vscode from 'vscode';
import type { Client } from '../lsp/client.js';

/**
 * Scaffold a project from a Typst Universe template — P4-14.
 *
 * A template package is an ordinary package whose `typst.toml` carries a
 * `[template]` section naming a directory to copy and an entry point to open.
 * The download and cache machinery is already in place for `#import
 * "@preview/…"`, so this is the UX on top of it: ask which template, ask where
 * to put it, copy, open.
 *
 * The copy is deliberately refuse-if-not-empty. A scaffolder that writes into a
 * directory with files already in it is a scaffolder that eventually overwrites
 * someone's work.
 */

/** The `[template]` section of a package manifest. */
interface TemplateInfo {
  /** The directory inside the package to copy. */
  path: string;
  /** The file to open afterwards, relative to `path`. */
  entrypoint: string;
}

/** What the server reports about a template package. */
interface TemplateResult {
  /** The resolved package directory. */
  root: string;
  /** Its `[template]` section, if it has one. */
  template?: TemplateInfo;
}

/** A few templates worth offering by name, so the picker is not an empty box. */
const SUGGESTIONS = [
  { spec: '@preview/charged-ieee:0.1.4', label: 'IEEE conference paper' },
  { spec: '@preview/modern-cv:0.9.0', label: 'CV / résumé' },
  { spec: '@preview/touying:0.6.1', label: 'Presentation slides' },
  { spec: '@preview/basic-report:0.3.0', label: 'Report' },
];

/** Ask for a template and a destination, then scaffold it. */
export async function createFromTemplate(client: Client): Promise<void> {
  const spec = await pickTemplate();
  if (!spec) return;

  const destination = await pickDestination();
  if (!destination) return;

  const result = await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title: `Typst: fetching ${spec}` },
    async () => client.request<TemplateResult>('typst/template', { spec }),
  );

  if (!result) {
    void vscode.window.showErrorMessage(
      `Typst: could not fetch ${spec}. Check the package name, and that ` +
        '`typstUltra.packages.enabled` is on.',
    );
    return;
  }
  if (!result.template) {
    void vscode.window.showErrorMessage(
      `Typst: \`${spec}\` is a package, not a template — it has no \`[template]\` section.`,
    );
    return;
  }

  try {
    await copyTemplate(result, destination);
  } catch (error) {
    void vscode.window.showErrorMessage(`Typst: ${String(error)}`);
    return;
  }

  const entry = vscode.Uri.joinPath(destination, result.template.entrypoint);
  await vscode.commands.executeCommand('vscode.open', entry);
}

async function pickTemplate(): Promise<string | undefined> {
  const items: (vscode.QuickPickItem & { spec?: string })[] = [
    ...SUGGESTIONS.map((entry) => ({
      label: entry.label,
      description: entry.spec,
      spec: entry.spec,
    })),
    { label: 'Another package…', description: 'Enter a Universe package spec' },
  ];

  const choice = await vscode.window.showQuickPick(items, {
    title: 'Typst: new project from template',
    placeHolder: 'Which template?',
  });
  if (!choice) return undefined;
  if (choice.spec) return choice.spec;

  return vscode.window.showInputBox({
    title: 'Typst: template package',
    prompt: 'A Universe template package',
    placeHolder: '@preview/charged-ieee:0.1.4',
    validateInput: (value) =>
      /^@[a-z0-9-]+\/[a-z0-9-]+:\d+\.\d+\.\d+$/.test(value.trim())
        ? undefined
        : 'Expected a spec like @preview/name:1.0.0',
  });
}

async function pickDestination(): Promise<vscode.Uri | undefined> {
  const chosen = await vscode.window.showOpenDialog({
    title: 'Typst: where should the project go?',
    canSelectFiles: false,
    canSelectFolders: true,
    canSelectMany: false,
    openLabel: 'Create here',
    defaultUri: vscode.workspace.workspaceFolders?.[0]?.uri,
  });
  return chosen?.[0];
}

/** Copy a template's files, refusing to write into a directory with content. */
async function copyTemplate(
  result: TemplateResult,
  destination: vscode.Uri,
): Promise<void> {
  const existing = await vscode.workspace.fs.readDirectory(destination);
  const visible = existing.filter(([name]) => !name.startsWith('.'));
  if (visible.length > 0) {
    throw new Error(
      `${path.basename(destination.fsPath)} is not empty. Choose an empty folder ` +
        'so nothing already there is overwritten.',
    );
  }

  const source = vscode.Uri.joinPath(
    vscode.Uri.file(result.root),
    result.template!.path,
  );
  await copyDirectory(source, destination);
}

async function copyDirectory(from: vscode.Uri, to: vscode.Uri): Promise<void> {
  await vscode.workspace.fs.createDirectory(to);

  for (const [name, type] of await vscode.workspace.fs.readDirectory(from)) {
    const source = vscode.Uri.joinPath(from, name);
    const target = vscode.Uri.joinPath(to, name);

    if (type === vscode.FileType.Directory) {
      await copyDirectory(source, target);
    } else {
      await vscode.workspace.fs.copy(source, target, { overwrite: false });
    }
  }
}
