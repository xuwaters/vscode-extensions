// JSON shapes mirrored from crates/json-analyzer/src/wasm_api.rs.
// The Rust side is the source of truth; keep the two files in sync by hand.

export interface LineCol {
  readonly line: number;
  readonly col: number;
}

export type DiagnosticSeverity = 'error' | 'warning' | 'info' | 'hint';

export interface AnalyzerDiagnostic {
  readonly code: string;
  readonly severity: DiagnosticSeverity;
  readonly message: string;
  readonly start: LineCol;
  readonly end: LineCol;
}

export type SymbolValueKind = 'object' | 'array' | 'string' | 'number' | 'boolean' | 'null';

export interface AnalyzerDocumentSymbol {
  readonly name: string;
  readonly detail: string;
  readonly kind: SymbolValueKind;
  readonly range_start: LineCol;
  readonly range_end: LineCol;
  readonly selection_start: LineCol;
  readonly selection_end: LineCol;
  readonly children: readonly AnalyzerDocumentSymbol[];
}

export interface AnalyzerFoldingRange {
  readonly start_line: number;
  readonly end_line: number;
  readonly kind: 'Region' | 'Comment';
}

export interface AnalyzerHover {
  readonly contents: string;
  readonly start: LineCol;
  readonly end: LineCol;
}

export interface AnalyzerTextEdit {
  readonly start: LineCol;
  readonly end: LineCol;
  readonly new_text: string;
}

/** Mirrors `FormatOptions` in crates/json-analyzer/src/features/formatting.rs. */
export interface FormatOptions {
  readonly tab_size: number;
  readonly insert_spaces: boolean;
  readonly sort_keys: boolean;
  readonly insert_final_newline: boolean;
}

export interface JsonlRow {
  readonly line: number;
  readonly cells: readonly string[];
}

export interface JsonlTable {
  readonly columns: readonly string[];
  readonly rows: readonly JsonlRow[];
  readonly total: number;
  readonly truncated: boolean;
}

/** The raw WASM handle: every method takes/returns strings. */
export interface WasmAnalyzer {
  update_file(uri: string, source: string, languageId: string): void;
  remove_file(uri: string): void;
  diagnostics(uri: string): string;
  document_symbols(uri: string): string;
  folding_ranges(uri: string): string;
  hover(uri: string, line: number, col: number): string;
  formatting(uri: string, optionsJson: string): string;
  sort_keys(uri: string, optionsJson: string): string;
  jsonl_table(uri: string, maxRows: number): string;
}

export interface WasmModule {
  Analyzer: new () => WasmAnalyzer;
}
