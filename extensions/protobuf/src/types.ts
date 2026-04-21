// JSON shapes mirrored from crates/proto3-analyzer/src/wasm_api.rs.

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

export interface AnalyzerWorkspaceSymbolItem {
  name: string;
  fqn: string;
  kind: string;
  file: string;
  range: { start: number; end: number };
  detail: string | null;
}

export interface AnalyzerLocation {
  file: string;
  start: LineCol;
  end: LineCol;
}

export interface AnalyzerHover {
  markdown: string;
  start: LineCol;
  end: LineCol;
}

export interface AnalyzerCompletionItem {
  label: string;
  insert_text: string;
  kind: string;
  detail: string;
}

export interface AnalyzerFoldingRange {
  start_line: number;
  end_line: number;
  kind: string;
}

export interface AnalyzerChangedFiles {
  affected: string[];
}

// Shape of the WASM export surface.
export interface WasmAnalyzerCtor {
  new (): WasmAnalyzer;
}

export interface WasmAnalyzer {
  set_include_paths(paths_json: string): void;
  update_file(uri: string, source: string): string;
  remove_file(uri: string): void;
  diagnostics(uri: string): string;
  document_symbols(uri: string): string;
  workspace_symbols(query: string): string;
  completion(uri: string, line: number, col: number): string;
  hover(uri: string, line: number, col: number): string;
  definition(uri: string, line: number, col: number): string;
  folding_ranges(uri: string): string;
  references(uri: string, line: number, col: number): string;
  rename(uri: string, line: number, col: number, newName: string): string;
}

export interface WasmModule {
  Analyzer: WasmAnalyzerCtor;
}
