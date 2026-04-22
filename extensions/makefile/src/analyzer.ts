import * as path from 'path';
import * as fs from 'fs';
import type {
  AnalyzerDiagnostic,
  AnalyzerDocumentSymbol,
  AnalyzerFoldingRange,
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
    const entry = path.join(extensionPath, 'wasm', 'makefile_analyzer.js');
    if (!fs.existsSync(entry)) {
      console.warn(
        'makefile-analyzer WASM bundle not found. Run `pnpm run build:wasm` in extensions/makefile.',
      );
      return new AnalyzerBridge(null);
    }
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const mod = require(entry) as WasmModule;
      return new AnalyzerBridge(new mod.Analyzer());
    } catch (e) {
      console.error('Failed to load makefile-analyzer WASM:', e);
      return new AnalyzerBridge(null);
    }
  }

  get ready(): boolean {
    return this.analyzer !== null;
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

  foldingRanges(uri: string): AnalyzerFoldingRange[] {
    if (!this.analyzer) return [];
    try {
      return JSON.parse(this.analyzer.folding_ranges(uri)) as AnalyzerFoldingRange[];
    } catch {
      return [];
    }
  }
}
