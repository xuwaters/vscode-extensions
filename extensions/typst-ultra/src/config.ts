import * as path from 'path';
import * as vscode from 'vscode';

/** The settings the server reads, in the shape it deserializes. */
export interface ServerSettings {
  compile: { when: string; debounce: number };
  diagnostics: { enabled: boolean };
  semanticTokens: string;
  formatter: { mode: string; printWidth: number; indentSize: number };
  inlayHints: { enabled: boolean };
  memory: { evictAge: number; restartThresholdMb: number };
}

/** The settings the host acts on itself. */
export interface HostSettings {
  rootPath: string;
  mainFile: string;
  fonts: { system: boolean; paths: string[] };
  packages: { enabled: boolean; registry: string; cachePath: string };
  preview: {
    scrollSync: 'both' | 'editorToPreview' | 'previewToEditor' | 'off';
    cursorIndicator: boolean;
    invertColors: 'never' | 'always' | 'auto';
    background: 'editor' | 'white' | 'gray';
    renderMode: 'svg' | 'png' | 'auto';
  };
  export: { outputPath: string };
  trace: string;
}

/** Everything under `typstUltra.`. */
export interface Config {
  server: ServerSettings;
  host: HostSettings;
}

/**
 * Settings whose change requires a whole new session rather than a recompile.
 *
 * Fonts, packages, and the compile root are baked into the `World` at
 * construction, so changing one means building a new one.
 */
const RESTART_KEYS = [
  'typstUltra.rootPath',
  'typstUltra.fonts.system',
  'typstUltra.fonts.paths',
  'typstUltra.packages.cachePath',
];

/** Read the current configuration for a workspace folder. */
export function read(scope?: vscode.Uri): Config {
  const config = vscode.workspace.getConfiguration('typstUltra', scope);

  return {
    server: {
      compile: {
        when: config.get('compile.when', 'onType'),
        debounce: config.get('compile.debounce', 150),
      },
      diagnostics: { enabled: config.get('diagnostics.enabled', true) },
      semanticTokens: config.get('semanticTokens', 'enable'),
      formatter: {
        mode: config.get('formatter.mode', 'typstyle'),
        printWidth: config.get('formatter.printWidth', 80),
        indentSize: config.get('formatter.indentSize', 2),
      },
      inlayHints: { enabled: config.get('inlayHints.enabled', false) },
      memory: {
        evictAge: config.get('memory.evictAge', 1),
        restartThresholdMb: config.get('memory.restartThresholdMb', 1024),
      },
    },
    host: {
      rootPath: config.get('rootPath', ''),
      mainFile: config.get('mainFile', ''),
      fonts: {
        system: config.get('fonts.system', true),
        paths: config.get('fonts.paths', [] as string[]),
      },
      packages: {
        enabled: config.get('packages.enabled', true),
        registry: config.get('packages.registry', 'https://packages.typst.org'),
        cachePath: config.get('packages.cachePath', ''),
      },
      preview: {
        scrollSync: config.get('preview.scrollSync', 'both'),
        cursorIndicator: config.get('preview.cursorIndicator', true),
        invertColors: config.get('preview.invertColors', 'never'),
        background: config.get('preview.background', 'editor'),
        renderMode: config.get('preview.renderMode', 'svg'),
      },
      export: { outputPath: config.get('export.outputPath', '$dir/$name') },
      trace: config.get('trace.server', 'off'),
    },
  };
}

/** Whether a configuration change needs the server restarted. */
export function needsRestart(event: vscode.ConfigurationChangeEvent): boolean {
  return RESTART_KEYS.some((key) => event.affectsConfiguration(key));
}

/**
 * The compile root: `typstUltra.rootPath` if set, otherwise the workspace
 * folder, otherwise the focused document's directory.
 *
 * This determines what an absolute path inside a document means, so getting it
 * from the workspace rather than the file is deliberate — a chapter in
 * `chapters/` must resolve `/assets/logo.svg` against the project.
 */
export function resolveRoot(config: Config, document?: vscode.Uri): string {
  if (config.host.rootPath) {
    return path.isAbsolute(config.host.rootPath)
      ? config.host.rootPath
      : path.join(workspaceRoot(document) ?? process.cwd(), config.host.rootPath);
  }
  return workspaceRoot(document) ?? path.dirname(document?.fsPath ?? process.cwd());
}

function workspaceRoot(document?: vscode.Uri): string | undefined {
  if (document) {
    const folder = vscode.workspace.getWorkspaceFolder(document);
    if (folder) return folder.uri.fsPath;
  }
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
}
