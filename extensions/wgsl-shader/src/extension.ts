import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import {
  HINT_ACTIONS,
  HINT_SETTING,
  RUST_ANALYZER_EXTENSION_ID,
  RUST_STRING_TOKENS_SETTING,
  hintActions,
  shouldOfferStringTokenFix,
} from './rustHint';

// ── WASM module interface ──────────────────────────────────────────

interface WasmModule {
  validate_wgsl(source: string): string;
  get_wgsl_tree(source: string): string;
}

interface ValidationResult {
  ok: boolean;
  errors: Array<{
    message: string;
    line: number;
    col: number;
    length: number;
  }>;
}

interface WgslTree {
  types: string[];
  global_variables: string[];
  functions: string[];
}

// ── WASM loader ────────────────────────────────────────────────────

function loadWasm(extensionPath: string): WasmModule | null {
  const wasmEntryPath = path.join(extensionPath, 'wasm', 'wgsl_analyzer.js');
  if (!fs.existsSync(wasmEntryPath)) {
    return null;
  }
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    return require(wasmEntryPath) as WasmModule;
  } catch (e) {
    console.error('Failed to load WGSL WASM module:', e);
    return null;
  }
}

// ── Validation ─────────────────────────────────────────────────────

function validateDocument(
  wasm: WasmModule,
  document: vscode.TextDocument,
  diagCollection: vscode.DiagnosticCollection,
): void {
  if (document.languageId !== 'wgsl') return;

  const source = document.getText();
  let result: ValidationResult;
  try {
    result = JSON.parse(wasm.validate_wgsl(source));
  } catch {
    return;
  }

  diagCollection.delete(document.uri);

  if (result.ok) return;

  const diagnostics: vscode.Diagnostic[] = result.errors.map((err) => {
    const line = Math.max(0, err.line - 1);
    const col = Math.max(0, err.col - 1);
    const len = Math.max(1, err.length);
    const range = new vscode.Range(line, col, line, col + len);
    return new vscode.Diagnostic(range, err.message, vscode.DiagnosticSeverity.Error);
  });

  diagCollection.set(document.uri, diagnostics);
}

// ── Completion provider ────────────────────────────────────────────

const WGSL_BUILTIN_FUNCTIONS = [
  // math
  'abs', 'acos', 'acosh', 'asin', 'asinh', 'atan', 'atanh', 'atan2',
  'ceil', 'clamp', 'cos', 'cosh', 'countLeadingZeros', 'countOneBits',
  'countTrailingZeros', 'cross', 'degrees', 'determinant', 'distance',
  'dot', 'exp', 'exp2', 'extractBits', 'faceForward', 'firstLeadingBit',
  'firstTrailingBit', 'floor', 'fma', 'fract', 'frexp', 'insertBits',
  'inverseSqrt', 'ldexp', 'length', 'log', 'log2', 'max', 'min', 'mix',
  'modf', 'normalize', 'pow', 'quantizeToF16', 'radians', 'reflect',
  'refract', 'reverseBits', 'round', 'saturate', 'sign', 'sin', 'sinh',
  'smoothstep', 'sqrt', 'step', 'tan', 'tanh', 'transpose', 'trunc',
  // texture
  'textureDimensions', 'textureGather', 'textureGatherCompare',
  'textureLoad', 'textureNumLayers', 'textureNumLevels',
  'textureNumSamples', 'textureSample', 'textureSampleBias',
  'textureSampleCompare', 'textureSampleCompareLevel',
  'textureSampleGrad', 'textureSampleLevel', 'textureStore',
  // atomic
  'atomicLoad', 'atomicStore', 'atomicAdd', 'atomicSub', 'atomicMax',
  'atomicMin', 'atomicAnd', 'atomicOr', 'atomicXor', 'atomicExchange',
  'atomicCompareExchangeWeak',
  // data packing
  'pack2x16float', 'pack2x16snorm', 'pack2x16unorm', 'pack4x8snorm',
  'pack4x8unorm', 'unpack2x16float', 'unpack2x16snorm',
  'unpack2x16unorm', 'unpack4x8snorm', 'unpack4x8unorm',
  // synchronization
  'storageBarrier', 'workgroupBarrier', 'workgroupUniformLoad',
  // construction / conversion
  'bitcast', 'select', 'arrayLength',
];

const WGSL_BUILTIN_TYPES = [
  'bool', 'f16', 'f32', 'i32', 'u32',
  'vec2', 'vec3', 'vec4',
  'vec2i', 'vec3i', 'vec4i', 'vec2u', 'vec3u', 'vec4u',
  'vec2f', 'vec3f', 'vec4f', 'vec2h', 'vec3h', 'vec4h',
  'mat2x2', 'mat2x3', 'mat2x4', 'mat3x2', 'mat3x3', 'mat3x4',
  'mat4x2', 'mat4x3', 'mat4x4',
  'mat2x2f', 'mat2x3f', 'mat2x4f', 'mat3x2f', 'mat3x3f', 'mat3x4f',
  'mat4x2f', 'mat4x3f', 'mat4x4f',
  'mat2x2h', 'mat2x3h', 'mat2x4h', 'mat3x2h', 'mat3x3h', 'mat3x4h',
  'mat4x2h', 'mat4x3h', 'mat4x4h',
  'array', 'atomic', 'ptr',
  'sampler', 'sampler_comparison',
  'texture_1d', 'texture_2d', 'texture_2d_array', 'texture_3d',
  'texture_cube', 'texture_cube_array', 'texture_multisampled_2d',
  'texture_storage_1d', 'texture_storage_2d', 'texture_storage_2d_array',
  'texture_storage_3d', 'texture_depth_2d', 'texture_depth_2d_array',
  'texture_depth_cube', 'texture_depth_multisampled_2d', 'texture_external',
];

const WGSL_KEYWORDS = [
  'fn', 'let', 'var', 'const', 'override', 'struct', 'alias',
  'if', 'else', 'for', 'while', 'loop', 'break', 'continue', 'continuing',
  'return', 'discard', 'switch', 'case', 'default', 'fallthrough',
  'enable', 'requires', 'diagnostic', 'const_assert',
  'true', 'false',
];

const WGSL_ATTRIBUTES = [
  'align', 'binding', 'builtin', 'compute', 'const', 'diagnostic',
  'fragment', 'group', 'id', 'interpolate', 'invariant', 'location',
  'must_use', 'size', 'vertex', 'workgroup_size',
];

function buildStaticCompletions(): vscode.CompletionItem[] {
  const items: vscode.CompletionItem[] = [];

  for (const fn of WGSL_BUILTIN_FUNCTIONS) {
    const item = new vscode.CompletionItem(fn, vscode.CompletionItemKind.Function);
    item.detail = 'WGSL built-in function';
    items.push(item);
  }
  for (const ty of WGSL_BUILTIN_TYPES) {
    const item = new vscode.CompletionItem(ty, vscode.CompletionItemKind.Class);
    item.detail = 'WGSL type';
    items.push(item);
  }
  for (const kw of WGSL_KEYWORDS) {
    const item = new vscode.CompletionItem(kw, vscode.CompletionItemKind.Keyword);
    item.detail = 'WGSL keyword';
    items.push(item);
  }
  for (const attr of WGSL_ATTRIBUTES) {
    const item = new vscode.CompletionItem(`@${attr}`, vscode.CompletionItemKind.Property);
    item.detail = 'WGSL attribute';
    item.insertText = `@${attr}`;
    items.push(item);
  }

  return items;
}

class WgslCompletionProvider implements vscode.CompletionItemProvider {
  private wasm: WasmModule | null;
  private staticItems: vscode.CompletionItem[];

  constructor(wasm: WasmModule | null) {
    this.wasm = wasm;
    this.staticItems = buildStaticCompletions();
  }

  provideCompletionItems(
    document: vscode.TextDocument,
  ): vscode.CompletionItem[] {
    const items = [...this.staticItems];

    if (this.wasm) {
      try {
        const tree: WgslTree = JSON.parse(this.wasm.get_wgsl_tree(document.getText()));
        for (const fn of tree.functions) {
          const item = new vscode.CompletionItem(fn, vscode.CompletionItemKind.Function);
          item.detail = 'user function';
          items.push(item);
        }
        for (const v of tree.global_variables) {
          const item = new vscode.CompletionItem(v, vscode.CompletionItemKind.Variable);
          item.detail = 'global variable';
          items.push(item);
        }
        for (const t of tree.types) {
          const item = new vscode.CompletionItem(t, vscode.CompletionItemKind.Class);
          item.detail = 'user type';
          items.push(item);
        }
      } catch {
        // parse failed, just return static items
      }
    }

    return items;
  }
}

// ── Document symbol provider ───────────────────────────────────────

class WgslDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  provideDocumentSymbols(document: vscode.TextDocument): vscode.DocumentSymbol[] {
    const symbols: vscode.DocumentSymbol[] = [];
    const fnRegex = /\bfn\s+([A-Za-z0-9_]+)\s*\(/;
    const structRegex = /\bstruct\s+([A-Za-z0-9_]+)/;

    for (let i = 0; i < document.lineCount; i++) {
      const line = document.lineAt(i);
      const text = line.text;

      let match = fnRegex.exec(text);
      if (match) {
        symbols.push(
          new vscode.DocumentSymbol(
            match[1],
            '',
            vscode.SymbolKind.Function,
            line.range,
            line.range,
          ),
        );
        continue;
      }

      match = structRegex.exec(text);
      if (match) {
        symbols.push(
          new vscode.DocumentSymbol(
            match[1],
            '',
            vscode.SymbolKind.Struct,
            line.range,
            line.range,
          ),
        );
      }
    }

    return symbols;
  }
}

// ── Embedded WGSL in Rust ──────────────────────────────────────────

/**
 * Offer, once per session, to turn off the rust-analyzer setting that hides the
 * WGSL highlighting inside tagged Rust strings. Asked only when a Rust file
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
      'rust-analyzer highlights whole string literals, which hides the WGSL colouring in /* wgsl */ strings. ' +
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
    vscode.workspace.onDidOpenTextDocument((doc) => {
      void consider(doc);
    }),
  );

  // The file that triggered activation is already open.
  const activeDoc = vscode.window.activeTextEditor?.document;
  if (activeDoc) void consider(activeDoc);
}

// ── Activation ─────────────────────────────────────────────────────

export function activate(context: vscode.ExtensionContext): void {
  registerRustHighlightHint(context);

  const wasm = loadWasm(context.extensionPath);

  if (!wasm) {
    console.warn(
      'WGSL WASM module not found. Validation and completion from user symbols are disabled. ' +
      'Run "pnpm run build:wasm" in extensions/wgsl-shader to build it.',
    );
  }

  // Symbol provider (works without WASM)
  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      { scheme: 'file', language: 'wgsl' },
      new WgslDocumentSymbolProvider(),
    ),
  );

  // Completion provider
  const config = vscode.workspace.getConfiguration('wgsl');
  if (config.get<boolean>('completion.enabled', true)) {
    context.subscriptions.push(
      vscode.languages.registerCompletionItemProvider('wgsl', new WgslCompletionProvider(wasm)),
    );
  }

  // Validation (requires WASM)
  if (wasm) {
    const diagCollection = vscode.languages.createDiagnosticCollection('wgsl');
    context.subscriptions.push(diagCollection);

    // Validate on save
    if (config.get<boolean>('validate.onSave', true)) {
      context.subscriptions.push(
        vscode.workspace.onDidSaveTextDocument((doc) => {
          validateDocument(wasm, doc, diagCollection);
        }),
      );
    }

    // Validate on type
    if (config.get<boolean>('validate.onType', false)) {
      let debounceTimer: ReturnType<typeof setTimeout> | undefined;
      context.subscriptions.push(
        vscode.workspace.onDidChangeTextDocument((e) => {
          if (debounceTimer) clearTimeout(debounceTimer);
          debounceTimer = setTimeout(() => {
            validateDocument(wasm, e.document, diagCollection);
          }, 300);
        }),
      );
    }

    // Validate command
    context.subscriptions.push(
      vscode.commands.registerCommand('wgsl.validateFile', () => {
        const document = vscode.window.activeTextEditor?.document;
        if (document) {
          validateDocument(wasm, document, diagCollection);
        }
      }),
    );

    // Validate on open
    context.subscriptions.push(
      vscode.workspace.onDidOpenTextDocument((doc) => {
        validateDocument(wasm, doc, diagCollection);
      }),
    );

    // Validate currently open editor
    const activeDoc = vscode.window.activeTextEditor?.document;
    if (activeDoc) {
      validateDocument(wasm, activeDoc, diagCollection);
    }

    // Clear diagnostics when a document is closed
    context.subscriptions.push(
      vscode.workspace.onDidCloseTextDocument((doc) => {
        diagCollection.delete(doc.uri);
      }),
    );
  }
}

export function deactivate(): void {}
