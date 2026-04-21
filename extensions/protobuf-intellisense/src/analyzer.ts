import * as path from 'path';
import * as fs from 'fs';
import type {
  AnalyzerDiagnostic,
  AnalyzerDocumentSymbol,
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
        'proto3-analyzer WASM bundle not found. Run `pnpm run build:wasm` in extensions/protobuf-intellisense.',
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
}
