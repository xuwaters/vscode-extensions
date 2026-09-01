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
import {
  GLSL_BUILTIN_FUNCTIONS,
  GLSL_BUILTIN_TYPES,
  GLSL_BUILTIN_VARIABLES,
  GLSL_DIRECTIVES,
  GLSL_KEYWORDS,
  GLSL_LAYOUT_QUALIFIERS,
  WGSL_ATTRIBUTES,
  WGSL_BUILTIN_FUNCTIONS,
  WGSL_BUILTIN_TYPES,
  WGSL_KEYWORDS,
} from './shaderData';
import { findGlslSymbols, findWgslSymbols, type ShaderSymbol } from './symbols';

// ── WASM module interface ──────────────────────────────────────────

interface WasmModule {
  validate_wgsl(source: string): string;
  get_wgsl_tree(source: string): string;
  validate_glsl(source: string, extension: string): string;
  get_glsl_tree(source: string, extension: string): string;
  glsl_shader_info(source: string, extension: string): string;
}

interface GlslShaderInfo {
  /** `vertex`, `fragment`, `compute`, or `unsupported`. */
  stage: string;
  /** Why validation was skipped, when it was. */
  skipped?: string;
}

interface ValidationResult {
  ok: boolean;
  errors: Array<{
    message: string;
    line: number;
    col: number;
    length: number;
  }>;
  /** GLSL only: the stage the source was parsed as, or `unsupported`. */
  stage?: string;
}

interface ShaderTree {
  types: string[];
  global_variables: string[];
  functions: string[];
}

/** The two languages this extension owns. */
type ShaderLanguage = 'wgsl' | 'glsl';

function shaderLanguage(document: vscode.TextDocument): ShaderLanguage | null {
  return document.languageId === 'wgsl' || document.languageId === 'glsl'
    ? document.languageId
    : null;
}

/** A document's file extension without the dot, lower-cased; `''` when it has none. */
function fileExtension(document: vscode.TextDocument): string {
  return path.extname(document.uri.path).replace(/^\./, '').toLowerCase();
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
    console.error('Failed to load shader WASM module:', e);
    return null;
  }
}

// ── Validation ─────────────────────────────────────────────────────

function validate(wasm: WasmModule, document: vscode.TextDocument): ValidationResult | null {
  const language = shaderLanguage(document);
  if (!language) return null;

  const source = document.getText();
  try {
    const json =
      language === 'wgsl'
        ? wasm.validate_wgsl(source)
        : wasm.validate_glsl(source, fileExtension(document));
    return JSON.parse(json) as ValidationResult;
  } catch {
    return null;
  }
}

function validateDocument(
  wasm: WasmModule,
  document: vscode.TextDocument,
  diagCollection: vscode.DiagnosticCollection,
): void {
  const result = validate(wasm, document);
  if (!result) return;

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

function buildWgslCompletions(): vscode.CompletionItem[] {
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

function buildGlslCompletions(): vscode.CompletionItem[] {
  const items: vscode.CompletionItem[] = [];

  for (const fn of GLSL_BUILTIN_FUNCTIONS) {
    const item = new vscode.CompletionItem(fn, vscode.CompletionItemKind.Function);
    item.detail = 'GLSL built-in function';
    items.push(item);
  }
  for (const ty of GLSL_BUILTIN_TYPES) {
    const item = new vscode.CompletionItem(ty, vscode.CompletionItemKind.Class);
    item.detail = 'GLSL type';
    items.push(item);
  }
  for (const kw of GLSL_KEYWORDS) {
    const item = new vscode.CompletionItem(kw, vscode.CompletionItemKind.Keyword);
    item.detail = 'GLSL keyword';
    items.push(item);
  }
  for (const variable of GLSL_BUILTIN_VARIABLES) {
    const item = new vscode.CompletionItem(variable.name, vscode.CompletionItemKind.Variable);
    item.detail = `GLSL built-in — ${variable.stages}`;
    items.push(item);
  }
  for (const qualifier of GLSL_LAYOUT_QUALIFIERS) {
    const item = new vscode.CompletionItem(qualifier, vscode.CompletionItemKind.Property);
    item.detail = 'GLSL layout qualifier';
    items.push(item);
  }
  for (const directive of GLSL_DIRECTIVES) {
    const item = new vscode.CompletionItem(`#${directive}`, vscode.CompletionItemKind.Keyword);
    item.detail = 'GLSL preprocessor directive';
    item.insertText = `#${directive}`;
    items.push(item);
  }

  return items;
}

class ShaderCompletionProvider implements vscode.CompletionItemProvider {
  private wasm: WasmModule | null;
  private language: ShaderLanguage;
  private staticItems: vscode.CompletionItem[];

  constructor(language: ShaderLanguage, wasm: WasmModule | null) {
    this.language = language;
    this.wasm = wasm;
    this.staticItems = language === 'wgsl' ? buildWgslCompletions() : buildGlslCompletions();
  }

  provideCompletionItems(document: vscode.TextDocument): vscode.CompletionItem[] {
    const items = [...this.staticItems];

    if (this.wasm) {
      try {
        const json =
          this.language === 'wgsl'
            ? this.wasm.get_wgsl_tree(document.getText())
            : this.wasm.get_glsl_tree(document.getText(), fileExtension(document));
        const tree: ShaderTree = JSON.parse(json);
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

const SYMBOL_KINDS: Record<ShaderSymbol['kind'], vscode.SymbolKind> = {
  function: vscode.SymbolKind.Function,
  struct: vscode.SymbolKind.Struct,
  variable: vscode.SymbolKind.Variable,
};

class ShaderDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  private language: ShaderLanguage;

  constructor(language: ShaderLanguage) {
    this.language = language;
  }

  provideDocumentSymbols(document: vscode.TextDocument): vscode.DocumentSymbol[] {
    const find = this.language === 'wgsl' ? findWgslSymbols : findGlslSymbols;
    return find(document.getText()).map((symbol) => {
      const range = document.lineAt(symbol.line).range;
      return new vscode.DocumentSymbol(
        symbol.name,
        '',
        SYMBOL_KINDS[symbol.kind],
        range,
        range,
      );
    });
  }
}

// ── GLSL shader stage ──────────────────────────────────────────────

const UNSUPPORTED_STAGE = 'unsupported';

function glslInfo(wasm: WasmModule, document: vscode.TextDocument): GlslShaderInfo | null {
  if (document.languageId !== 'glsl') return null;
  try {
    return JSON.parse(wasm.glsl_shader_info(document.getText(), fileExtension(document)));
  } catch {
    return null;
  }
}

/**
 * GLSL sources carry no record of their own stage, and naga needs one before it
 * can parse at all — so show which one was picked, and say so out loud when the
 * file is outside the dialect naga implements and gets no diagnostics at all.
 */
function registerStageStatusBar(context: vscode.ExtensionContext, wasm: WasmModule): void {
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  status.command = 'glsl.showShaderStage';
  context.subscriptions.push(status);

  function update(editor: vscode.TextEditor | undefined): void {
    const enabled = vscode.workspace
      .getConfiguration('glsl')
      .get<boolean>('showStageInStatusBar', true);
    const info = enabled && editor ? glslInfo(wasm, editor.document) : null;
    if (!info) {
      status.hide();
      return;
    }
    const label = info.stage === UNSUPPORTED_STAGE ? 'stage unknown' : info.stage;
    status.text = info.skipped ? `GLSL: ${label} (not validated)` : `GLSL: ${label}`;
    status.tooltip = info.skipped
      ? `${info.skipped}. This file is highlighted but not validated.`
      : `Validated as a ${info.stage} shader. Add #pragma shader_stage(…) to override.`;
    status.show();
  }

  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(update),
    vscode.workspace.onDidSaveTextDocument(() => update(vscode.window.activeTextEditor)),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration('glsl.showStageInStatusBar')) {
        update(vscode.window.activeTextEditor);
      }
    }),
  );

  update(vscode.window.activeTextEditor);
}

// ── Embedded shaders in Rust ───────────────────────────────────────

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
    vscode.workspace.onDidOpenTextDocument((doc) => {
      void consider(doc);
    }),
  );

  // The file that triggered activation is already open.
  const activeDoc = vscode.window.activeTextEditor?.document;
  if (activeDoc) void consider(activeDoc);
}

// ── Activation ─────────────────────────────────────────────────────

const LANGUAGES: ShaderLanguage[] = ['wgsl', 'glsl'];

/** Whether validation of `document`'s language should run for this trigger. */
function validationEnabled(document: vscode.TextDocument, trigger: 'onSave' | 'onType'): boolean {
  const language = shaderLanguage(document);
  if (!language) return false;
  return vscode.workspace
    .getConfiguration(language)
    .get<boolean>(`validate.${trigger}`, trigger === 'onSave');
}

export function activate(context: vscode.ExtensionContext): void {
  registerRustHighlightHint(context);

  const wasm = loadWasm(context.extensionPath);

  if (!wasm) {
    console.warn(
      'Shader WASM module not found. Validation and completion from user symbols are disabled. ' +
      'Run "pnpm run build:wasm" in extensions/wgsl-shader to build it.',
    );
  }

  for (const language of LANGUAGES) {
    // Symbol provider (works without WASM)
    context.subscriptions.push(
      vscode.languages.registerDocumentSymbolProvider(
        { scheme: 'file', language },
        new ShaderDocumentSymbolProvider(language),
      ),
    );

    // Completion provider
    if (vscode.workspace.getConfiguration(language).get<boolean>('completion.enabled', true)) {
      context.subscriptions.push(
        vscode.languages.registerCompletionItemProvider(
          language,
          new ShaderCompletionProvider(language, wasm),
        ),
      );
    }
  }

  // Validation (requires WASM)
  if (!wasm) return;

  const diagCollection = vscode.languages.createDiagnosticCollection('shader');
  context.subscriptions.push(diagCollection);

  // Validate on save
  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (validationEnabled(doc, 'onSave')) validateDocument(wasm, doc, diagCollection);
    }),
  );

  // Validate on type
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  context.subscriptions.push(
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (!validationEnabled(e.document, 'onType')) return;
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        validateDocument(wasm, e.document, diagCollection);
      }, 300);
    }),
  );

  // Validate commands
  for (const command of ['wgsl.validateFile', 'glsl.validateFile']) {
    context.subscriptions.push(
      vscode.commands.registerCommand(command, () => {
        const document = vscode.window.activeTextEditor?.document;
        if (document) {
          validateDocument(wasm, document, diagCollection);
        }
      }),
    );
  }

  // Report the stage a GLSL file is validated as
  context.subscriptions.push(
    vscode.commands.registerCommand('glsl.showShaderStage', () => {
      const document = vscode.window.activeTextEditor?.document;
      const info = document ? glslInfo(wasm, document) : null;
      if (!info) {
        void vscode.window.showInformationMessage('The active file is not a GLSL shader.');
      } else if (info.skipped) {
        void vscode.window.showInformationMessage(
          `${info.skipped}, so this file is highlighted but not validated.` +
          (info.stage === UNSUPPORTED_STAGE
            ? ' Add #pragma shader_stage(vertex|fragment|compute) to validate it as one of those.'
            : ''),
        );
      } else {
        void vscode.window.showInformationMessage(
          `This file is validated as a ${info.stage} shader. ` +
          'The stage comes from #pragma shader_stage(…), then the file extension, ' +
          'then the built-ins the source uses.',
        );
      }
    }),
  );

  registerStageStatusBar(context, wasm);

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

export function deactivate(): void {}
