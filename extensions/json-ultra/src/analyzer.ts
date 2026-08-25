// Loads the WASM analyzer and wraps its string-typed surface in typed,
// infallible methods. A missing or broken bundle degrades to a null
// bridge: the extension still activates, providers return nothing, and
// the console says why.

import * as fs from 'fs';
import * as path from 'path';
import type {
  AnalyzerDiagnostic,
  AnalyzerDocumentSymbol,
  AnalyzerFoldingRange,
  AnalyzerHover,
  AnalyzerTextEdit,
  FormatOptions,
  JsonlTable,
  WasmAnalyzer,
  WasmModule,
} from './types.js';

export class AnalyzerBridge {
  private constructor(private readonly analyzer: WasmAnalyzer | null) {}

  static load(extensionPath: string): AnalyzerBridge {
    const entry = path.join(extensionPath, 'wasm', 'json_analyzer.js');
    if (!fs.existsSync(entry)) {
      console.warn(
        'json-analyzer WASM bundle not found. Run `pnpm run build:wasm` in extensions/json-ultra.',
      );
      return new AnalyzerBridge(null);
    }
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const mod = require(entry) as WasmModule;
      return new AnalyzerBridge(new mod.Analyzer());
    } catch (error) {
      console.error('Failed to load json-analyzer WASM:', error);
      return new AnalyzerBridge(null);
    }
  }

  static empty(): AnalyzerBridge {
    return new AnalyzerBridge(null);
  }

  get available(): boolean {
    return this.analyzer !== null;
  }

  updateFile(uri: string, source: string, languageId: string): void {
    if (!this.analyzer) return;
    try {
      this.analyzer.update_file(uri, source, languageId);
    } catch {
      // A panic here poisons nothing; the next query just misses.
    }
  }

  removeFile(uri: string): void {
    if (!this.analyzer) return;
    try {
      this.analyzer.remove_file(uri);
    } catch {
      /* ignore */
    }
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

  hover(uri: string, line: number, col: number): AnalyzerHover | null {
    if (!this.analyzer) return null;
    try {
      return JSON.parse(this.analyzer.hover(uri, line, col)) as AnalyzerHover | null;
    } catch {
      return null;
    }
  }

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

  sortKeys(uri: string, options: FormatOptions): AnalyzerTextEdit | null {
    if (!this.analyzer) return null;
    try {
      return JSON.parse(
        this.analyzer.sort_keys(uri, JSON.stringify(options)),
      ) as AnalyzerTextEdit | null;
    } catch {
      return null;
    }
  }

  jsonlTable(uri: string, maxRows: number): JsonlTable | null {
    if (!this.analyzer) return null;
    try {
      return JSON.parse(this.analyzer.jsonl_table(uri, maxRows)) as JsonlTable | null;
    } catch {
      return null;
    }
  }
}
