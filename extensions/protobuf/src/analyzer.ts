import * as path from 'path';
import * as fs from 'fs';
import type {
  AnalyzerCompletionItem,
  AnalyzerDiagnostic,
  AnalyzerDocumentSymbol,
  AnalyzerFoldingRange,
  AnalyzerHover,
  AnalyzerLocation,
  AnalyzerRange,
  AnalyzerTextEdit,
  AnalyzerWorkspaceSymbolItem,
  WasmAnalyzer,
  WasmModule,
} from './types';

/**
 * Thin wrapper around the WASM `Analyzer` handle. Owns one long-lived
 * instance for the extension host and translates the `JSON in, JSON out`
 * surface into typed callers.
 */
export class AnalyzerBridge {
  private analyzer: WasmAnalyzer | null;

  private constructor(analyzer: WasmAnalyzer | null) {
    this.analyzer = analyzer;
  }

  static load(extensionPath: string): AnalyzerBridge {
    const entry = path.join(extensionPath, 'wasm', 'proto3_analyzer.js');
    if (!fs.existsSync(entry)) {
      console.warn(
        'proto3-analyzer WASM bundle not found. Run `pnpm run build:wasm` in extensions/protobuf.',
      );
      return new AnalyzerBridge(null);
    }
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const mod = require(entry) as WasmModule;
      return new AnalyzerBridge(new mod.Analyzer());
    } catch (e) {
      console.error('Failed to load proto3-analyzer WASM:', e);
      return new AnalyzerBridge(null);
    }
  }

  get ready(): boolean {
    return this.analyzer !== null;
  }

  setIncludePaths(paths: string[]): void {
    this.analyzer?.set_include_paths(JSON.stringify(paths));
  }

  updateFile(uri: string, source: string): void {
    this.analyzer?.update_file(uri, source);
  }

  removeFile(uri: string): void {
    this.analyzer?.remove_file(uri);
  }

  diagnostics(uri: string): AnalyzerDiagnostic[] {
    if (!this.analyzer) return [];
    try {
      return JSON.parse(this.analyzer.diagnostics(uri)) as AnalyzerDiagnostic[];
    } catch {
      return [];
    }
  }

  documentSymbols(uri: string): AnalyzerDocumentSymbol[] {
    if (!this.analyzer) return [];
    try {
      return JSON.parse(this.analyzer.document_symbols(uri)) as AnalyzerDocumentSymbol[];
    } catch {
      return [];
    }
  }

  workspaceSymbols(query: string): AnalyzerWorkspaceSymbolItem[] {
    if (!this.analyzer) return [];
    try {
      return JSON.parse(this.analyzer.workspace_symbols(query)) as AnalyzerWorkspaceSymbolItem[];
    } catch {
      return [];
    }
  }

  definition(uri: string, line: number, col: number): AnalyzerLocation | null {
    if (!this.analyzer) return null;
    try {
      return JSON.parse(this.analyzer.definition(uri, line, col)) as AnalyzerLocation | null;
    } catch {
      return null;
    }
  }

  hover(uri: string, line: number, col: number): AnalyzerHover | null {
    if (!this.analyzer) return null;
    try {
      return JSON.parse(this.analyzer.hover(uri, line, col)) as AnalyzerHover | null;
    } catch {
      return null;
    }
  }

  completion(uri: string, line: number, col: number): AnalyzerCompletionItem[] {
    if (!this.analyzer) return [];
    try {
      return JSON.parse(this.analyzer.completion(uri, line, col)) as AnalyzerCompletionItem[];
    } catch {
      return [];
    }
  }

  foldingRanges(uri: string): AnalyzerFoldingRange[] {
    if (!this.analyzer) return [];
    try {
      return JSON.parse(this.analyzer.folding_ranges(uri)) as AnalyzerFoldingRange[];
    } catch {
      return [];
    }
  }

  references(uri: string, line: number, col: number, includeDeclaration: boolean): AnalyzerTextEdit[] {
    if (!this.analyzer) return [];
    try {
      return JSON.parse(
        this.analyzer.references(uri, line, col, includeDeclaration),
      ) as AnalyzerTextEdit[];
    } catch {
      return [];
    }
  }

  prepareRename(uri: string, line: number, col: number): AnalyzerRange | null {
    if (!this.analyzer) return null;
    try {
      return JSON.parse(this.analyzer.prepare_rename(uri, line, col)) as AnalyzerRange | null;
    } catch {
      return null;
    }
  }

  rename(uri: string, line: number, col: number, newName: string): AnalyzerTextEdit[] | null {
    if (!this.analyzer) return null;
    try {
      return JSON.parse(this.analyzer.rename(uri, line, col, newName)) as AnalyzerTextEdit[] | null;
    } catch {
      return null;
    }
  }
}
