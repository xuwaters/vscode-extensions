import * as fs from 'fs';
import * as path from 'path';
import type {
  AnalyzerDiagnostic,
  AnalyzerDocumentSymbol,
  AnalyzerFoldingRange,
  AnalyzerHover,
  AnalyzerRange,
  AnalyzerTextEdit,
  FormatOptions,
  LineCol,
  WasmAnalyzer,
  WasmModule,
} from './types';

/**
 * Thin wrapper around the WASM `Analyzer` handle. Owns one long-lived
 * instance for the extension host and turns its `JSON in, JSON out` surface
 * into typed calls that never throw: a panic crosses the WASM boundary as an
 * exception, and a missing or broken `wasm/` must not take the extension host
 * down with it.
 */
export class AnalyzerBridge {
  private constructor(private readonly analyzer: WasmAnalyzer | null) {}

  static load(extensionPath: string): AnalyzerBridge {
    const entry = path.join(extensionPath, 'wasm', 'ron_analyzer.js');
    if (!fs.existsSync(entry)) {
      console.warn(
        'ron-analyzer WASM bundle not found. Run `pnpm run build:wasm` in extensions/ron-ultra.',
      );
      return new AnalyzerBridge(null);
    }
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const mod = require(entry) as WasmModule;
      return new AnalyzerBridge(new mod.Analyzer());
    } catch (e) {
      console.error('Failed to load ron-analyzer WASM:', e);
      return new AnalyzerBridge(null);
    }
  }

  get ready(): boolean {
    return this.analyzer !== null;
  }

  updateFile(uri: string, source: string): void {
    try {
      this.analyzer?.update_file(uri, source);
    } catch (e) {
      console.error('ron-analyzer update_file failed:', e);
    }
  }

  removeFile(uri: string): void {
    try {
      this.analyzer?.remove_file(uri);
    } catch {
      // Nothing to clean up on our side.
    }
  }

  diagnostics(uri: string): AnalyzerDiagnostic[] {
    return this.call((a) => a.diagnostics(uri), []);
  }

  documentSymbols(uri: string): AnalyzerDocumentSymbol[] {
    return this.call((a) => a.document_symbols(uri), []);
  }

  foldingRanges(uri: string): AnalyzerFoldingRange[] {
    return this.call((a) => a.folding_ranges(uri), []);
  }

  hover(uri: string, line: number, col: number): AnalyzerHover | null {
    return this.call((a) => a.hover(uri, line, col), null);
  }

  /** One innermost-first chain of ranges per position. */
  selectionRanges(uri: string, positions: LineCol[]): AnalyzerRange[][] {
    return this.call((a) => a.selection_ranges(uri, JSON.stringify(positions)), []);
  }

  /** `null` when the file is already formatted or the formatter declined. */
  formatting(uri: string, options: FormatOptions): AnalyzerTextEdit | null {
    return this.call((a) => a.formatting(uri, JSON.stringify(options)), null);
  }

  private call<T>(f: (a: WasmAnalyzer) => string, fallback: T): T {
    if (!this.analyzer) return fallback;
    try {
      return (JSON.parse(f(this.analyzer)) as T | null) ?? fallback;
    } catch (e) {
      console.error('ron-analyzer call failed:', e);
      return fallback;
    }
  }
}
