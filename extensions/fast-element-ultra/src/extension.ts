/**
 * The extension host side. Contains no analysis (architecture.md §1): it
 * forwards `fastElementUltra.*` settings into the TypeScript server plugin,
 * draws colour swatches inside FAST templates, runs the workspace-analysis
 * command against the plugin over tsserver's protocol, and shows a language
 * status item for the engine's state.
 */

import * as vscode from 'vscode';

const PLUGIN_NAME = 'wx-fast-element-tsplugin';
const ANALYZE_REQUEST = '_fast-element-ultra:analyze';
const STATUS_REQUEST = '_fast-element-ultra:status';

const SELECTOR: vscode.DocumentSelector = [
  { language: 'typescript' },
  { language: 'typescriptreact' },
  { language: 'javascript' },
  { language: 'javascriptreact' },
];

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  await configurePlugin();
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(async (event) => {
      if (
        event.affectsConfiguration('fastElementUltra') ||
        event.affectsConfiguration('html.experimental.customData')
      ) {
        await configurePlugin();
      }
    }),
  );

  context.subscriptions.push(
    vscode.languages.registerColorProvider(SELECTOR, new TemplateColorProvider()),
  );

  const diagnostics = vscode.languages.createDiagnosticCollection('fast-element-ultra');
  context.subscriptions.push(diagnostics);
  context.subscriptions.push(
    vscode.commands.registerCommand('fastElementUltra.analyze', () =>
      analyzeWorkspace(diagnostics),
    ),
    vscode.commands.registerCommand('fastElementUltra.clearAnalysis', () => diagnostics.clear()),
  );

  const status = vscode.languages.createLanguageStatusItem('fastElementUltra.status', SELECTOR);
  status.name = 'FAST Element Ultra';
  status.command = {
    title: 'Analyze Workspace',
    command: 'fastElementUltra.analyze',
  };
  context.subscriptions.push(status);
  void refreshStatus(status);
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(() => void refreshStatus(status)),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration('fastElementUltra')) void refreshStatus(status);
    }),
  );
}

async function configurePlugin(): Promise<void> {
  const tsExtension = vscode.extensions.getExtension('vscode.typescript-language-features');
  if (!tsExtension) return;
  await tsExtension.activate();
  const api = tsExtension.exports?.getAPI?.(0);
  if (!api) return;
  const settings = vscode.workspace.getConfiguration('fastElementUltra');
  const htmlCustomData = vscode.workspace
    .getConfiguration('html')
    .get<unknown>('experimental.customData');
  api.configurePlugin(PLUGIN_NAME, {
    disable: settings.get('disable'),
    strict: settings.get('strict'),
    logging: settings.get('logging'),
    dontShowSuggestions: settings.get('dontShowSuggestions'),
    htmlTemplateTags: settings.get('htmlTemplateTags'),
    cssTemplateTags: settings.get('cssTemplateTags'),
    maxProjectImportDepth: settings.get('maxProjectImportDepth'),
    maxNodeModuleImportDepth: settings.get('maxNodeModuleImportDepth'),
    globalTags: settings.get('globalTags'),
    globalAttributes: settings.get('globalAttributes'),
    globalEvents: settings.get('globalEvents'),
    customHtmlData: settings.get('customHtmlData'),
    htmlCustomData,
    rules: settings.get('rules'),
  });
}

// ------------------------------------------------------- workspace analysis

interface AnalyzeResponseDiagnostic {
  file: string;
  start: { line: number; character: number };
  end: { line: number; character: number };
  message: string;
  severity: 'error' | 'warning' | 'suggestion';
  ruleId: string;
}

async function analyzeWorkspace(collection: vscode.DiagnosticCollection): Promise<void> {
  await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: 'FAST Element Ultra: analyzing workspace templates',
    },
    async () => {
      // The plugin needs a loaded project; open documents drive tsserver, so
      // touch one FAST file first if none is open.
      const body = await tsserverRequest<AnalyzeResponseDiagnostic[]>(ANALYZE_REQUEST, {});
      if (body === undefined) {
        void vscode.window.showWarningMessage(
          'FAST Element Ultra: tsserver did not answer. Open a TypeScript file that uses @microsoft/fast-element first, then run the command again.',
        );
        return;
      }
      collection.clear();
      const byFile = new Map<string, vscode.Diagnostic[]>();
      for (const item of body) {
        const range = new vscode.Range(
          item.start.line,
          item.start.character,
          item.end.line,
          item.end.character,
        );
        const severity =
          item.severity === 'error'
            ? vscode.DiagnosticSeverity.Error
            : item.severity === 'warning'
              ? vscode.DiagnosticSeverity.Warning
              : vscode.DiagnosticSeverity.Hint;
        const diagnostic = new vscode.Diagnostic(range, item.message, severity);
        diagnostic.source = 'fast-element-ultra';
        diagnostic.code = item.ruleId;
        const list = byFile.get(item.file) ?? [];
        list.push(diagnostic);
        byFile.set(item.file, list);
      }
      for (const [file, diagnostics] of byFile) {
        collection.set(vscode.Uri.file(file), diagnostics);
      }
      const total = body.length;
      void vscode.window.showInformationMessage(
        total === 0
          ? 'FAST Element Ultra: no problems found in workspace templates.'
          : `FAST Element Ultra: ${total} problem${total === 1 ? '' : 's'} reported to the Problems panel.`,
      );
    },
  );
}

async function tsserverRequest<T>(command: string, args: unknown): Promise<T | undefined> {
  try {
    const response = (await vscode.commands.executeCommand(
      'typescript.tsserverRequest',
      command,
      args,
    )) as { body?: T } | undefined;
    return response?.body;
  } catch {
    return undefined;
  }
}

async function refreshStatus(status: vscode.LanguageStatusItem): Promise<void> {
  const disabled = vscode.workspace
    .getConfiguration('fastElementUltra')
    .get<boolean>('disable');
  if (disabled) {
    status.text = 'FAST: off';
    status.detail = 'fastElementUltra.disable is set';
    status.severity = vscode.LanguageStatusSeverity.Warning;
    return;
  }
  const body = await tsserverRequest<{ engineState: string; tsVersion: string }>(
    STATUS_REQUEST,
    {},
  );
  if (!body) {
    status.text = 'FAST';
    status.detail = 'FAST Element analysis (state unknown until a TypeScript file is open)';
    status.severity = vscode.LanguageStatusSeverity.Information;
    return;
  }
  switch (body.engineState) {
    case 'ok':
      status.text = 'FAST: on';
      status.detail = `FAST Element analysis active (TypeScript ${body.tsVersion})`;
      status.severity = vscode.LanguageStatusSeverity.Information;
      break;
    case 'poisoned':
      status.text = 'FAST: engine stopped';
      status.detail = 'The analysis engine failed and was stopped; TypeScript is unaffected. Restart the TS server to retry.';
      status.severity = vscode.LanguageStatusSeverity.Error;
      break;
    default:
      status.text = `FAST: ${body.engineState}`;
      status.detail = 'FAST Element analysis is not running';
      status.severity = vscode.LanguageStatusSeverity.Warning;
      break;
  }
}

// --------------------------------------------------------- colour swatches

/**
 * Colours inside `html\`\`` and `css\`\`` literals. The engine lives in the
 * other process, so the template ranges are re-derived here with the same
 * substitution rules — on literal parts only, never inside `${…}`
 * (design/features.md §11).
 */
class TemplateColorProvider implements vscode.DocumentColorProvider {
  provideDocumentColors(document: vscode.TextDocument): vscode.ColorInformation[] {
    const text = document.getText();
    const out: vscode.ColorInformation[] = [];
    for (const segment of literalTemplateSegments(text)) {
      findColors(text, segment.start, segment.end, (start, end, color) => {
        out.push(
          new vscode.ColorInformation(
            new vscode.Range(document.positionAt(start), document.positionAt(end)),
            color,
          ),
        );
      });
    }
    return out;
  }

  provideColorPresentations(
    color: vscode.Color,
    context: { document: vscode.TextDocument; range: vscode.Range },
  ): vscode.ColorPresentation[] {
    const toHex = (v: number): string =>
      Math.round(v * 255)
        .toString(16)
        .padStart(2, '0');
    const label =
      color.alpha < 1
        ? `#${toHex(color.red)}${toHex(color.green)}${toHex(color.blue)}${toHex(color.alpha)}`
        : `#${toHex(color.red)}${toHex(color.green)}${toHex(color.blue)}`;
    void context;
    return [new vscode.ColorPresentation(label)];
  }
}

interface Segment {
  start: number;
  end: number;
}

/** The literal (non-`${…}`) parts of every fast-looking tagged template. */
function literalTemplateSegments(text: string): Segment[] {
  const out: Segment[] = [];
  const open = /\b(?:html|css)\s*(?:<[^`\n]{0,200}?>)?\s*`/g;
  let match: RegExpExecArray | null;
  while ((match = open.exec(text)) !== null) {
    let pos = match.index + match[0].length;
    let literalStart = pos;
    let depth = 0;
    while (pos < text.length) {
      const ch = text[pos];
      if (depth === 0 && ch === '\\') {
        pos += 2;
        continue;
      }
      if (depth === 0 && ch === '`') {
        out.push({ start: literalStart, end: pos });
        pos += 1;
        break;
      }
      if (depth === 0 && ch === '$' && text[pos + 1] === '{') {
        out.push({ start: literalStart, end: pos });
        depth = 1;
        pos += 2;
        continue;
      }
      if (depth > 0) {
        if (ch === '{') depth += 1;
        else if (ch === '}') {
          depth -= 1;
          if (depth === 0) literalStart = pos + 1;
        }
      }
      pos += 1;
    }
    open.lastIndex = pos;
  }
  return out;
}

const COLOR_PATTERN =
  /#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})\b|\brgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*([\d.]+)\s*)?\)/g;

function findColors(
  text: string,
  start: number,
  end: number,
  emit: (start: number, end: number, color: vscode.Color) => void,
): void {
  const slice = text.slice(start, end);
  let match: RegExpExecArray | null;
  COLOR_PATTERN.lastIndex = 0;
  while ((match = COLOR_PATTERN.exec(slice)) !== null) {
    const token = match[0];
    let color: vscode.Color | undefined;
    if (token.startsWith('#')) {
      color = parseHexColor(token);
    } else if (match[1] !== undefined) {
      color = new vscode.Color(
        Number(match[1]) / 255,
        Number(match[2]) / 255,
        Number(match[3]) / 255,
        match[4] !== undefined ? Number(match[4]) : 1,
      );
    }
    if (color) emit(start + match.index, start + match.index + token.length, color);
  }
}

function parseHexColor(token: string): vscode.Color | undefined {
  const hex = token.slice(1);
  const expand = (s: string): number => parseInt(s.length === 1 ? s + s : s, 16) / 255;
  if (hex.length === 3 || hex.length === 4) {
    return new vscode.Color(
      expand(hex[0]),
      expand(hex[1]),
      expand(hex[2]),
      hex.length === 4 ? expand(hex[3]) : 1,
    );
  }
  if (hex.length === 6 || hex.length === 8) {
    return new vscode.Color(
      expand(hex.slice(0, 2)),
      expand(hex.slice(2, 4)),
      expand(hex.slice(4, 6)),
      hex.length === 8 ? expand(hex.slice(6, 8)) : 1,
    );
  }
  return undefined;
}

export function deactivate(): void {
  // Nothing to dispose beyond subscriptions.
}
