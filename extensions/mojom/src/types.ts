// JSON shapes mirrored from crates/mojom-analyzer/src/wasm_api.rs.

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

export interface AnalyzerHover {
  markdown: string;
  start: LineCol;
  end: LineCol;
}

export interface AnalyzerLocation {
  file: string;
  start: LineCol;
  end: LineCol;
}

export interface AnalyzerCompletionItem {
  label: string;
  insert_text: string;
  kind: string;
  detail: string;
}

export interface AnalyzerWorkspaceSymbol {
  name: string;
  fqn: string;
  kind: string;
  file: string;
  start: LineCol;
  end: LineCol;
  detail: string | null;
}

export interface WasmAnalyzer {
  set_include_paths(json: string): void;
  update_file(uri: string, source: string): void;
  preload_file(uri: string, source: string): void;
  remove_file(uri: string): void;
  diagnostics(uri: string): string;
  document_symbols(uri: string): string;
  folding_ranges(uri: string): string;
  hover(uri: string, line: number, col: number): string;
  definition(uri: string, line: number, col: number): string;
  completion(uri: string, line: number, col: number): string;
  workspace_symbols(query: string): string;
}

export interface WasmModule {
  Analyzer: new () => WasmAnalyzer;
}
