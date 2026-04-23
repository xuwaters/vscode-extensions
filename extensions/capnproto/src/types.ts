// JSON shapes mirrored from crates/capnp-analyzer/src/wasm_api.rs.

export interface LineCol {
  line: number;
  col: number;
}

export interface AnalyzerDiagnostic {
  code: string;
  severity: 'error' | 'warning';
  message: string;
  start: LineCol;
  end: LineCol;
}

export interface AnalyzerDocumentSymbol {
  name: string;
  detail: string;
  kind: string;
  range_start: LineCol;
  range_end: LineCol;
  selection_start: LineCol;
  selection_end: LineCol;
  children: AnalyzerDocumentSymbol[];
}

export interface AnalyzerFoldingRange {
  start_line: number;
  end_line: number;
  kind: string;
}

export interface WasmAnalyzer {
  update_file(uri: string, source: string): void;
  remove_file(uri: string): void;
  diagnostics(uri: string): string;
  document_symbols(uri: string): string;
  folding_ranges(uri: string): string;
}

export interface WasmModule {
  Analyzer: new () => WasmAnalyzer;
}
