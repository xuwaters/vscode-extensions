import * as fs from 'fs';
import * as path from 'path';
import { pathToFileURL } from 'url';
import {
  createConnection,
  ProposedFeatures,
  type Connection,
  type InitializeParams,
  type InitializeResult,
} from 'vscode-languageserver/node';
import { FontIndex, systemFontDirs } from './fonts.js';
import { Packages, defaultCacheDir, readTemplate } from './packages.js';
import { listDir, readFile, type Roots } from './vfs.js';

/**
 * The Typst language server.
 *
 * Runs as a child process of the extension host, which is the one place this
 * repo departs from its usual "require the WASM straight into the extension
 * host" pattern. The reasons are a cold compile that blocks for up to ~260 ms,
 * a WASM heap that is never returned to the OS, and a compiler panic that would
 * otherwise take down every extension in the window.
 *
 * Everything here is glue. The interesting parts are in Rust, and are tested
 * there.
 */

/** The shape `wasm/typst_lsp_wasm.js` exposes. */
interface WasmModule {
  TypstServer: {
    new (host: HostServices, init: unknown): TypstServerInstance;
    indexFont(data: Uint8Array): { info: unknown; index: number }[];
    heapBytes(): number;
    typstVersion(): string;
  };
}

interface TypstServerInstance {
  capabilities(): unknown;
  onRequest(method: string, params: unknown): unknown;
  onNotification(method: string, params: unknown): void;
  drainEvents(): { method: string; params: unknown }[];
  setFontFaces(faces: { info: unknown; index: number }[]): void;
}

/**
 * The callbacks Rust makes back into Node.
 *
 * Every one of these is **synchronous** and callable from inside a compile.
 * Anything that cannot be — a package download, a font scan — is deferred and
 * followed by a recompile.
 */
interface HostServices {
  readFile(root: string, vpath: string): Uint8Array | null;
  listDir(root: string, vpath: string): string[];
  fontData(face: number): Uint8Array | null;
  resolvePackage(spec: string): string;
  now(): number;
  timezoneOffsetMinutes(): number;
}

/** What the extension host passes at startup. */
interface InitOptions {
  /** The compile root, as a filesystem path. */
  rootPath: string;
  /** The compile root, as a URI, as the client spells document URIs. */
  rootUri: string;
  /** The initial entry file, root-relative. */
  mainPath: string;
  /** Where the bundled fonts live, inside the extension. */
  bundledFontsPath: string;
  /** Where the font index cache is kept, in global storage. */
  fontCachePath: string;
  /** Extra font directories from `typstUltra.fonts.paths`. */
  extraFontPaths: string[];
  /** `typstUltra.fonts.system`. */
  systemFonts: boolean;
  /** `typstUltra.packages.*`. */
  packages: { enabled: boolean; registry: string; cachePath: string };
  /** The `typstUltra.*` settings the Rust side reads. */
  settings: Record<string, unknown>;
}

const connection: Connection = createConnection(ProposedFeatures.all);

/** Requests the Rust side answers. Anything else gets a method-not-found. */
const REQUESTS = [
  'textDocument/completion',
  'textDocument/hover',
  'textDocument/definition',
  'textDocument/references',
  'textDocument/prepareRename',
  'textDocument/rename',
  'textDocument/documentSymbol',
  'workspace/symbol',
  'textDocument/semanticTokens/full',
  'textDocument/semanticTokens/full/delta',
  'textDocument/foldingRange',
  'textDocument/selectionRange',
  'textDocument/documentLink',
  'textDocument/formatting',
  'textDocument/rangeFormatting',
  'textDocument/inlayHint',
  'textDocument/signatureHelp',
  'textDocument/codeAction',
  'textDocument/codeLens',
  'typst/renderPages',
  'typst/documentMetrics',
  'typst/jumpFromClick',
  'typst/jumpFromCursor',
  'typst/export',
] as const;

/** Notifications the Rust side handles. */
const NOTIFICATIONS = [
  'textDocument/didOpen',
  'textDocument/didChange',
  'textDocument/didSave',
  'textDocument/didClose',
  'workspace/didChangeConfiguration',
  'typst/setMain',
  'typst/workspaceFiles',
] as const;

/** Everything that outlives one request. */
interface Engine {
  server: TypstServerInstance;
  fonts: FontIndex;
  packages: Packages;
  roots: Roots;
  options: InitOptions;
  wasm: WasmModule;
}

let engine: Engine | null = null;
let compileTimer: NodeJS.Timeout | null = null;
let compileDebounceMs = 150;
let compileWhen: 'onType' | 'onSave' | 'never' = 'onType';
let lastActiveUri: string | undefined;

connection.onInitialize((params: InitializeParams): InitializeResult => {
  const options = (params.initializationOptions ?? {}) as Partial<InitOptions>;
  const resolved = withDefaults(options, params);

  let wasm: WasmModule;
  try {
    wasm = loadWasm();
  } catch (error) {
    // Graceful degradation, the same shape log-viewer uses: the server comes up
    // and says what to run, rather than the client seeing a crashed process.
    connection.console.error(
      'Typst engine not built. Run `pnpm run build:wasm` in extensions/typst-ultra.\n' +
        String(error),
    );
    connection.sendNotification('typst/engineMissing', {
      command: 'pnpm run build:wasm',
    });
    return { capabilities: {} };
  }

  applyScheduling(resolved.settings);

  const roots: Roots = {
    project: resolved.rootPath,
    packageCache: resolved.packages.cachePath || defaultCacheDir(),
  };

  const packages = new Packages({
    enabled: resolved.packages.enabled,
    registry: resolved.packages.registry,
    cache: roots.packageCache,
    onReady: () => scheduleCompile(0),
    onStatus: (spec, state) =>
      connection.sendNotification('typst/packageStatus', {
        spec,
        state: state.kind === 'failed' ? 'failed' : state.kind,
        error: state.kind === 'failed' ? state.reason : undefined,
      }),
  });

  const fonts = new FontIndex(resolved.fontCachePath, (data) =>
    wasm.TypstServer.indexFont(data),
  );

  // Bundled fonts first: small, and correct for the overwhelming majority of
  // documents. System fonts arrive in the background and cost one recompile.
  const bundled = fonts.addDirectories([resolved.bundledFontsPath]);
  connection.console.log(
    `indexed ${bundled.faces} bundled font faces in ${bundled.ms} ms ` +
      `(${bundled.parsed} parsed, ${bundled.faces - bundled.parsed} cached)`,
  );
  fonts.save();

  const host: HostServices = {
    readFile: (root, vpath) => readFile(roots, root, vpath),
    listDir: (root, vpath) => listDir(roots, root, vpath),
    fontData: (face) => fonts.data(face),
    resolvePackage: (spec) => packages.resolve(spec, roots),
    now: () => Date.now(),
    // `getTimezoneOffset` counts minutes *behind* UTC; typst wants minutes east.
    timezoneOffsetMinutes: () => -new Date().getTimezoneOffset(),
  };

  const server = new wasm.TypstServer(host, {
    rootUri: resolved.rootUri,
    packageCacheUri: pathToFileURL(roots.packageCache).toString(),
    mainPath: resolved.mainPath,
    settings: resolved.settings,
    fontFaces: fonts.descriptors,
    packages: [],
  });

  engine = { server, fonts, packages, roots, options: resolved, wasm };

  if (resolved.systemFonts || resolved.extraFontPaths.length > 0) {
    // Deferred to the next tick so `initialize` returns promptly; the scan is
    // synchronous once it starts, but by then the client is already live.
    setTimeout(() => indexMoreFonts(resolved), 0);
  }

  connection.console.log(`typst ${wasm.TypstServer.typstVersion()} engine ready`);

  return { capabilities: server.capabilities() as InitializeResult['capabilities'] };
});

for (const method of REQUESTS) {
  connection.onRequest(method, (params: unknown) => {
    if (!engine) return null;
    try {
      const result = engine.server.onRequest(method, params);
      drain();
      return result;
    } catch (error) {
      drain();
      throw error;
    }
  });
}

for (const method of NOTIFICATIONS) {
  connection.onNotification(method, (params: unknown) => {
    if (!engine) return;

    if (method === 'workspace/didChangeConfiguration') {
      applySettings(params);
    }

    engine.server.onNotification(method, params);
    drain();

    // The debounce timer lives here rather than in Rust, because WASM has no
    // runtime to hang one on.
    if (method === 'textDocument/didOpen' || method === 'textDocument/didChange') {
      lastActiveUri = uriOf(params) ?? lastActiveUri;
      if (compileWhen === 'onType') scheduleCompile(compileDebounceMs);
    } else if (method === 'textDocument/didSave') {
      lastActiveUri = uriOf(params) ?? lastActiveUri;
      if (compileWhen !== 'never') scheduleCompile(0);
    } else if (method === 'typst/setMain') {
      scheduleCompile(0);
    }
  });
}

connection.onNotification('typst/clearPackageCache', () => {
  engine?.packages.clearCache();
  scheduleCompile(0);
});

connection.onRequest('typst/heapBytes', () => engine?.wasm.TypstServer.heapBytes() ?? 0);

/**
 * `typst/template`: fetch a template package and describe it — P4-14.
 *
 * Handled here rather than in Rust because it needs the network and can afford
 * to wait, neither of which is true of the compile path.
 */
connection.onRequest('typst/template', async (params: { spec?: unknown }) => {
  if (!engine || typeof params?.spec !== 'string') return null;

  const root = await engine.packages.ensure(params.spec, engine.roots);
  if (!root) return null;

  let manifest = '';
  try {
    manifest = fs.readFileSync(path.join(root, 'typst.toml'), 'utf8');
  } catch {
    return { root };
  }

  const template = readTemplate(manifest);
  return template ? { root, template } : { root };
});

connection.onShutdown(() => {
  engine?.fonts.save();
});

connection.listen();

/** Send everything the Rust side queued while handling a message. */
function drain(): void {
  if (!engine) return;
  for (const event of engine.server.drainEvents()) {
    void connection.sendNotification(event.method, event.params);
  }
}

/** Run a compile after `delay` milliseconds of quiet. */
function scheduleCompile(delay: number): void {
  if (compileTimer) clearTimeout(compileTimer);
  compileTimer = setTimeout(() => {
    compileTimer = null;
    if (!engine) return;
    try {
      engine.server.onNotification('typst/compile', { uri: lastActiveUri });
    } catch (error) {
      connection.console.error(`compile failed: ${String(error)}`);
    }
    drain();
  }, delay);
}

/** Index system and user-configured font directories, then recompile. */
function indexMoreFonts(options: InitOptions): void {
  if (!engine) return;

  const dirs = [
    ...(options.systemFonts ? systemFontDirs() : []),
    ...options.extraFontPaths.map((dir) =>
      path.isAbsolute(dir) ? dir : path.join(options.rootPath, dir),
    ),
  ];
  if (dirs.length === 0) return;

  const before = engine.fonts.entries.length;
  const stats = engine.fonts.addDirectories(dirs);
  engine.fonts.save();

  if (engine.fonts.entries.length === before) return;

  connection.console.log(
    `indexed ${stats.faces - before} more font faces in ${stats.ms} ms ` +
      `(${stats.parsed} files parsed, the rest served from the cache)`,
  );

  // Rebuild the font book in place; the open documents stay where they are.
  engine.server.setFontFaces(engine.fonts.descriptors);
  connection.sendNotification('typst/fontsChanged', {
    faces: engine.fonts.entries.length,
    ms: stats.ms,
  });
  scheduleCompile(0);
}

/** Read the compile-scheduling settings out of a settings object. */
function applyScheduling(settings: Record<string, unknown>): void {
  const compile = settings.compile as
    | { when?: typeof compileWhen; debounce?: number }
    | undefined;
  if (compile?.when) compileWhen = compile.when;
  if (typeof compile?.debounce === 'number') compileDebounceMs = compile.debounce;
}

function applySettings(params: unknown): void {
  if (typeof params !== 'object' || params === null) return;
  const settings = (params as { settings?: Record<string, unknown> }).settings;
  const section = (settings?.typstUltra ?? settings) as
    | Record<string, unknown>
    | undefined;
  if (!section) return;

  applyScheduling(section);

  const packages = section.packages as
    | { enabled?: boolean; registry?: string }
    | undefined;
  if (packages && engine) {
    engine.packages.configure({
      enabled: packages.enabled ?? true,
      registry: packages.registry ?? 'https://packages.typst.org',
    });
    // A setting change is a reason to retry a download that failed.
    engine.packages.reset();
  }
}

function uriOf(params: unknown): string | undefined {
  if (typeof params !== 'object' || params === null) return undefined;
  const document = (params as { textDocument?: { uri?: unknown } }).textDocument;
  return typeof document?.uri === 'string' ? document.uri : undefined;
}

/** Fill in anything the client left out. */
function withDefaults(
  options: Partial<InitOptions>,
  params: InitializeParams,
): InitOptions {
  const rootUri =
    options.rootUri ??
    params.workspaceFolders?.[0]?.uri ??
    params.rootUri ??
    pathToFileURL(process.cwd()).toString();

  return {
    rootPath: options.rootPath ?? process.cwd(),
    rootUri,
    mainPath: options.mainPath ?? 'main.typ',
    bundledFontsPath: options.bundledFontsPath ?? '',
    fontCachePath: options.fontCachePath ?? '',
    extraFontPaths: options.extraFontPaths ?? [],
    systemFonts: options.systemFonts ?? true,
    packages: {
      enabled: options.packages?.enabled ?? true,
      registry: options.packages?.registry ?? 'https://packages.typst.org',
      cachePath: options.packages?.cachePath ?? '',
    },
    settings: options.settings ?? {},
  };
}

/**
 * Load the WASM engine.
 *
 * `dist/server.js` sits one level below the extension root, and `wasm/` sits
 * beside `dist/`.
 */
function loadWasm(): WasmModule {
  const entry = path.join(__dirname, '..', 'wasm', 'typst_lsp_wasm.js');
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  return require(entry) as WasmModule;
}
