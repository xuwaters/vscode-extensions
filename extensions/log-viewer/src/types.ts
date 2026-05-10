// Shared types between the host extension and the webview, plus the
// hand-written shape of the WASM module (mirrors crates/log-parser exports).

export interface ParsedLines {
  /** ANSI-rendered HTML per line (no trailing newlines). */
  html: string[];
  /** Plain text per line (escape sequences stripped). */
  text: string[];
}

export interface FilterRule {
  name: string;
  pattern: string;
  regex?: boolean;
  caseSensitive?: boolean;
  /** CSS color used as the line background highlight. */
  color?: string;
  /** Default-enabled state, may be overridden at runtime. */
  enabled?: boolean;
}

export type FilterMode = 'highlight' | 'only-matching';

export interface ViewState {
  renderAnsi: boolean;
  wordWrap: boolean;
  /** 0 = inherit editor default. */
  fontSize: number;
  filterMode: FilterMode;
}

// ===== host → webview =====

export interface InitMessage {
  type: 'init';
  lines: ParsedLines;
  /** length === lines.text.length; value = ruleIndex+1 (1-based) or 0 for no match. */
  filterMatches: number[];
  rules: FilterRule[];
  state: ViewState;
  truncated: boolean;
  totalBytes: number;
}

export interface UpdateMessage {
  type: 'update';
  lines?: ParsedLines;
  filterMatches?: number[];
  rules?: FilterRule[];
  state?: ViewState;
  truncated?: boolean;
  totalBytes?: number;
}

export interface FocusSearchMessage {
  type: 'focusSearch';
}

export interface CommandToggleMessage {
  type: 'commandToggle';
  /** Names map to webview-side handlers; see webview script. */
  key: 'renderAnsi' | 'wordWrap' | 'filterMode';
}

export interface FontSizeCommandMessage {
  type: 'fontSizeCommand';
  delta: number | 'reset';
}

export type HostToWebview =
  | InitMessage
  | UpdateMessage
  | FocusSearchMessage
  | CommandToggleMessage
  | FontSizeCommandMessage;

// ===== webview → host =====

export interface ReadyMessage {
  type: 'ready';
}

export interface SetStateMessage {
  type: 'setState';
  state: Partial<ViewState>;
}

export interface SetFilterEnabledMessage {
  type: 'setFilterEnabled';
  /** Index into the rules array. */
  index: number;
  enabled: boolean;
}

export interface OpenInTextMessage {
  type: 'openInText';
}

export type WebviewToHost =
  | ReadyMessage
  | SetStateMessage
  | SetFilterEnabledMessage
  | OpenInTextMessage;

// ===== WASM module shape (mirrors wasm/log_parser.d.ts) =====

export interface WasmLogIndex {
  free(): void;
  readonly lineCount: number;
  allLinesJson(): string;
  renderRange(start: number, end: number): string;
  matchFilters(rulesJson: string): Uint8Array;
  search(query: string, regex: boolean, caseSensitive: boolean): Uint32Array;
}

export interface WasmModule {
  init(): void;
  stripAnsi(input: string): string;
  LogIndex: new (text: string) => WasmLogIndex;
}
