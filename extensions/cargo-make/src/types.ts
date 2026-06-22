// JSON shapes mirrored from crates/cargo-make-analyzer/src/wasm_api.rs.

export interface LineCol {
  line: number;
  col: number;
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

export type AnalyzerCompletionKind = 'Field' | 'EnumValue' | 'Task' | 'Section';

export interface AnalyzerCompletionItem {
  label: string;
  detail: string | null;
  kind: AnalyzerCompletionKind;
  insert_text: string;
  replace_length: number;
}

export interface AnalyzerHover {
  contents: string;
  start: LineCol;
  end: LineCol;
}

export interface AnalyzerLocation {
  start: LineCol;
  end: LineCol;
}

export interface WasmAnalyzerCtor {
  new (): WasmAnalyzer;
}

export interface WasmAnalyzer {
  update_file(uri: string, source: string): void;
  remove_file(uri: string): void;
  diagnostics(uri: string): string;
  document_symbols(uri: string): string;
  folding_ranges(uri: string): string;
  complete(uri: string, line: number, col: number): string;
  hover(uri: string, line: number, col: number): string;
  definition(uri: string, line: number, col: number): string;
}

export interface WasmModule {
  Analyzer: WasmAnalyzerCtor;
}
