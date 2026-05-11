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

export interface FilterSet {
  name: string;
  description?: string;
  /** Default-enabled state, may be overridden at runtime per editor session. */
  enabled?: boolean;
  filters: FilterRule[];
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
  sets: FilterSet[];
  activeSetNames: string[];
  palette: string[];
  state: ViewState;
  truncated: boolean;
  totalBytes: number;
}

export interface UpdateMessage {
  type: 'update';
  lines?: ParsedLines;
  filterMatches?: number[];
  rules?: FilterRule[];
  sets?: FilterSet[];
  activeSetNames?: string[];
  palette?: string[];
  state?: ViewState;
  truncated?: boolean;
  totalBytes?: number;
}

export interface FilterConfigSavedMessage {
  type: 'filterConfigSaveResult';
  ok: boolean;
  error?: string;
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

export interface OpenFilterEditorMessage {
  type: 'openFilterEditor';
}

// ===== Streaming protocol =====

export interface LineRecord {
  html: string;
  text: string;
}

export interface IndexProgressFrame {
  scannedLines: number;
  scannedBytes: number;
  fileSize: number;
  complete: boolean;
  /** Set once indexing completes. */
  totalLines?: number;
}

export interface StreamInitMessage {
  type: 'streamInit';
  totalLines: number;
  /** Total bytes (from stat). */
  fileSize: number;
  /** Stride used by the index. */
  stride: number;
  indexProgress: IndexProgressFrame;
  rules: FilterRule[];
  sets: FilterSet[];
  activeSetNames: string[];
  palette: string[];
  state: ViewState;
}

export interface IndexProgressMessage {
  type: 'indexProgress';
  progress: IndexProgressFrame;
}

export interface WindowMessage {
  type: 'window';
  /** Webview request id this window answers (for stale-response dedup). */
  requestId?: number;
  start: number;
  /** Number of trailing lines that were unavailable due to incomplete index. */
  partialCount?: number;
  lines: LineRecord[];
}

export interface FilterProgressMessage {
  type: 'filterProgress';
  scannedBytes: number;
  scannedLines: number;
  /** Sparse hits since the last progress event. Each entry is `[line, rule]`. */
  hits: Array<[number, number]>;
}

export interface FilterDoneMessage {
  type: 'filterDone';
  totalHits: number;
  truncated: boolean;
}

export interface SearchProgressMessage {
  type: 'searchProgress';
  scannedBytes: number;
  scannedLines: number;
  /** Sparse line numbers added since the last progress event. */
  hits: number[];
}

export interface SearchDoneMessage {
  type: 'searchDone';
  totalHits: number;
  truncated: boolean;
}

export interface FileChangedMessage {
  type: 'fileChanged';
  previousSize: number;
  currentSize: number;
}

export type HostToWebview =
  | InitMessage
  | UpdateMessage
  | FocusSearchMessage
  | CommandToggleMessage
  | FontSizeCommandMessage
  | OpenFilterEditorMessage
  | FilterConfigSavedMessage
  | StreamInitMessage
  | IndexProgressMessage
  | WindowMessage
  | FilterProgressMessage
  | FilterDoneMessage
  | SearchProgressMessage
  | SearchDoneMessage
  | FileChangedMessage;

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

export interface SetActiveSetsMessage {
  type: 'setActiveSets';
  names: string[];
}

export interface SaveFilterConfigMessage {
  type: 'saveFilterConfig';
  sets: FilterSet[];
  palette: string[];
}

export interface OpenInTextMessage {
  type: 'openInText';
}

export interface RequestWindowMessage {
  type: 'requestWindow';
  /** Webview-assigned id echoed back on the response. */
  requestId: number;
  start: number;
  end: number;
}

export interface SetSearchMessage {
  type: 'setSearch';
  query: string;
  regex: boolean;
  caseSensitive: boolean;
}

export interface CancelSearchMessage {
  type: 'cancelSearch';
}

export interface ReloadMessage {
  type: 'reload';
}

export type WebviewToHost =
  | ReadyMessage
  | SetStateMessage
  | SetFilterEnabledMessage
  | SetActiveSetsMessage
  | SaveFilterConfigMessage
  | OpenInTextMessage
  | RequestWindowMessage
  | SetSearchMessage
  | CancelSearchMessage
  | ReloadMessage;

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
  /** Render a byte slab to `{ html, text }` JSON. */
  renderLines(bytes: Uint8Array): string;
  /** Per-line tags: 0 = no match, else 1-based rule index. */
  matchLines(bytes: Uint8Array, rulesJson: string): Uint8Array;
  /** Local line indices that match the query. */
  searchLines(
    bytes: Uint8Array,
    query: string,
    regex: boolean,
    caseSensitive: boolean,
  ): Uint32Array;
  /** Byte offsets of every `\n` in the slab. */
  findNewlines(bytes: Uint8Array): Uint32Array;
}
