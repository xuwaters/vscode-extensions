import * as path from 'path';
import * as fs from 'fs';
import type {
  AnalyzerCompletionItem,
  AnalyzerDiagnostic,
  AnalyzerDocumentSymbol,
  AnalyzerFoldingRange,
  AnalyzerHover,
  AnalyzerTextEdit,
  FormatOptions,
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
    const entry = path.join(extensionPath, 'wasm', 'dotenv_analyzer.js');
    if (!fs.existsSync(entry)) {
      console.warn(
        'dotenv-analyzer WASM bundle not found. Run `pnpm run build:wasm` in extensions/dotenv.',
      );
      return new AnalyzerBridge(null);
    }
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const mod = require(entry) as WasmModule;
      return new AnalyzerBridge(new mod.Analyzer());
    } catch (e) {
      console.error('Failed to load dotenv-analyzer WASM:', e);
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

  complete(uri: string, line: number, col: number): AnalyzerCompletionItem[] {
    if (!this.analyzer) return [];
    try {
      return JSON.parse(this.analyzer.complete(uri, line, col)) as AnalyzerCompletionItem[];
    } catch {
      return [];
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

  /** `null` when the file is already formatted or the formatter declined. */
  formatting(uri: string, options: FormatOptions): AnalyzerTextEdit | null {
    if (!this.analyzer) return null;
    try {
      return JSON.parse(
        this.analyzer.formatting(uri, JSON.stringify(options)),
      ) as AnalyzerTextEdit | null;
    } catch {
      return null;
    }
  }

  formattingRange(
    uri: string,
    startLine: number,
    endLine: number,
    options: FormatOptions,
  ): AnalyzerTextEdit | null {
    if (!this.analyzer) return null;
    try {
      return JSON.parse(
        this.analyzer.formatting_range(uri, startLine, endLine, JSON.stringify(options)),
      ) as AnalyzerTextEdit | null;
    } catch {
      return null;
    }
  }
}
