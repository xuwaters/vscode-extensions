// JSON shapes mirrored from crates/ron-ultra/ron-analyzer/src/wasm_api.rs.

export interface LineCol {
  line: number;
  col: number;
}

export interface AnalyzerRange {
  start: LineCol;
  end: LineCol;
}

export interface AnalyzerDiagnostic {
  code: string;
  severity: 'error' | 'warning' | 'info' | 'hint';
  message: string;
  start: LineCol;
  end: LineCol;
}

export interface AnalyzerDocumentSymbol {
  name: string;
  detail: string;
  /** A `vscode.SymbolKind` member name, such as `Struct` or `Field`. */
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
  kind: 'Comment' | 'Region';
}

export interface AnalyzerHover {
  contents: string;
  start: LineCol;
  end: LineCol;
}

export interface AnalyzerTextEdit {
  start: LineCol;
  end: LineCol;
  new_text: string;
}

/** Mirrors `FormatOptions` in crates/ron-ultra/ron-analyzer/src/features/formatting.rs. */
export interface FormatOptions {
  tab_size: number;
  insert_spaces: boolean;
  print_width: number;
  insert_final_newline: boolean;
}

export interface WasmAnalyzer {
  update_file(uri: string, source: string): void;
  remove_file(uri: string): void;
  diagnostics(uri: string): string;
  document_symbols(uri: string): string;
  folding_ranges(uri: string): string;
  hover(uri: string, line: number, col: number): string;
  selection_ranges(uri: string, positions_json: string): string;
  formatting(uri: string, options_json: string): string;
}

export interface WasmModule {
  Analyzer: new () => WasmAnalyzer;
}
