import * as fs from 'fs';
import * as path from 'path';

/** A position in engine coordinates: `col` is UTF-16 units, like VSCode. */
export interface EnginePos {
  line: number;
  col: number;
}

export type EngineMode = 'normal' | 'insert' | 'visual' | 'visualLine';

export interface EngineSelection {
  anchor: EnginePos;
  active: EnginePos;
}

export interface EngineEdit {
  start: EnginePos;
  end: EnginePos;
  text: string;
}

export type EngineCommand =
  | { kind: 'undo' }
  | { kind: 'redo' }
  | {
      kind: 'indentLines';
      startLine: number;
      endLine: number;
      dedent: boolean;
      /** How many shifts: more than one only for `:>>` and longer. */
      times: number;
    }
  | { kind: 'scroll'; to: 'center' | 'top' | 'bottom' }
  | { kind: 'openLine'; above: boolean }
  | { kind: 'showHover' }
  | EngineFileCommand
  /** `:e` / `:e!`: read the document back from disk. */
  | { kind: 'reload'; force: boolean }
  | { kind: 'scrollLines'; down: boolean; lines: number; moveCursor: boolean }
  | { kind: 'gotoDefinition' }
  /**
   * A yank or delete into `"+` / `"*`: put `text` on the system clipboard.
   * Linewise text ends in a line break. The other direction is a pull: see
   * `EngineSession.wantsClipboard`.
   */
  | { kind: 'setClipboard'; text: string }
  /** `:registers`, `:marks`, `:jumps`: a listing for the host to show. */
  | { kind: 'showList'; title: string; rows: EngineListRow[] }
  /**
   * `'A` / `` `A ``: file mark A is in another document. Open it with the
   * cursor at `line`/`col`, or on the line's first non-blank.
   */
  | { kind: 'gotoMark'; uri: string; line: number; col: number; firstNonBlank: boolean }
  /**
   * `@a`, and `.` after `o`/`O`: run `keys` (spelled as `key` takes them),
   * `count` times over, as though typed — each through `feed`, once the one
   * before it has landed. A run of plain characters in insert mode goes in
   * with `insertText` instead. A key that comes back `failed` stops the run.
   */
  | { kind: 'execute'; keys: string[]; count: number };

/** One row of a `showList`: `"a` / `'b`, a note beside it, its content. */
export interface EngineListRow {
  label: string;
  description: string;
  detail: string;
  /** A file mark in another document, which the row's content names. */
  uri?: string;
}

/** What a register holds (crates/vim-engine `Register`). */
export interface EngineRegister {
  text: string;
  linewise: boolean;
  /** One piece per cursor, when a multi-cursor key filled it. */
  parts?: string[];
}

/**
 * `:w :q :x :wq :wa :qa :up`, `ZZ`, `ZQ`: what was asked for. Whether the
 * document has unsaved changes — which only the host knows — decides what
 * actually happens.
 */
export interface EngineFileCommand {
  kind: 'file';
  /** `ifModified` is `:x` and `:up`, which write only when there are changes. */
  write: 'never' | 'always' | 'ifModified';
  /** Close the editor afterwards. */
  close: boolean;
  /** Every document (`:wa`, `:qa`, `:xa`). */
  all: boolean;
  /** `!`: close even with unsaved changes, discarding them. */
  force: boolean;
}

/**
 * Incremental-search UI state, present while a `/` or `?` prompt is open and
 * on the key that closes it. `active` carries flat [line, startCol, endCol]
 * triples to highlight plus the match to peek at; `committed` keeps the view
 * where the search landed; `cancelled` restores the pre-search view.
 */
export type EngineSearchUi =
  | { kind: 'active'; matches: number[]; current?: [number, number, number] }
  | { kind: 'committed' }
  | { kind: 'cancelled' };

/** One EasyMotion label: where it sits and what is left to type for it. */
export interface EngineEasyLabel {
  line: number;
  col: number;
  text: string;
}

/**
 * EasyMotion overlay state, on the key that paints the labels and on the one
 * that takes them down. `labels` also carries the visible line span they were
 * chosen from, which is what the host dims around them.
 */
export type EngineEasyUi =
  | { kind: 'labels'; labels: EngineEasyLabel[]; firstLine: number; lastLine: number }
  | { kind: 'done' };

/** One engine response (crates/vim-engine `Effects`). */
export interface Effects {
  mode: EngineMode;
  selections: EngineSelection[];
  edits: EngineEdit[];
  commands: EngineCommand[];
  pending: string;
  /** Status-bar report: ex counts and errors, a search that found nothing. */
  message?: string;
  /** Search-typing UI state; absent outside a `/`/`?` prompt's lifetime. */
  search?: EngineSearchUi;
  /** EasyMotion overlay; absent outside a jump's lifetime. */
  easy?: EngineEasyUi;
  /**
   * The key failed the way a Vim command beeps (a motion with nowhere to
   * go, a pattern not found): a register being run stops there.
   */
  failed?: boolean;
  /** The register a `q` is recording into, if one is. */
  recording?: string;
}

export interface EngineChange {
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
  text: string;
}

/** Shape of the wasm-bindgen `Session` handle. */
interface WasmSession {
  key(key: string): string;
  feed(key: string): string;
  insert_text(text: string): string;
  reset(text: string, line: number, col: number): void;
  apply_changes(changesJson: string): void;
  set_position(line: number, col: number): string;
  set_selection(
    anchorLine: number,
    anchorCol: number,
    activeLine: number,
    activeCol: number,
    byHand: boolean,
  ): string;
  set_cursors(selectionsJson: string, byHand: boolean): string;
  set_view(first: number, last: number): void;
  set_easy_motion(enabled: boolean, trigger: string, keys: string): void;
  set_document(uri: string): void;
  wants_clipboard(key: string): boolean;
  set_clipboard(text: string): void;
  register(name: string): string;
  set_register(name: string, text: string, linewise: boolean): boolean;
  take_edits(): Uint8Array;
  mode(): string;
  text(): string;
  free(): void;
}

/** `Effects` as the wasm layer sends it: edits held back, only counted. */
type WireEffects = Omit<Effects, 'edits'> & { editCount: number };

interface WasmModule {
  Session: new (text: string, line: number, col: number) => WasmSession;
}

/** What the controller needs of the bridge: one session per document. */
export interface SessionFactory {
  createSession(text: string, line: number, col: number): EngineSession | null;
}

/**
 * Loads the wasm-pack-built `vim_engine` module lazily and hands out
 * per-document sessions. Degrades gracefully when `wasm/` is missing —
 * typical when the extension runs before `pnpm run build:wasm`.
 */
export class EngineBridge implements SessionFactory {
  private module: WasmModule | null | undefined;

  constructor(private readonly extensionPath: string) {}

  get ready(): boolean {
    return this.loadModule() !== null;
  }

  createSession(text: string, line: number, col: number): EngineSession | null {
    const mod = this.loadModule();
    return mod ? new EngineSession(new mod.Session(text, line, col)) : null;
  }

  private loadModule(): WasmModule | null {
    if (this.module !== undefined) return this.module;
    const entry = path.join(this.extensionPath, 'wasm', 'vim_engine.js');
    if (!fs.existsSync(entry)) {
      console.warn(
        'vim-engine WASM bundle not found. Run `pnpm run build:wasm` in extensions/vim-ultra.',
      );
      this.module = null;
      return null;
    }
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      this.module = require(entry) as WasmModule;
    } catch (e) {
      console.error('Failed to load vim-engine WASM:', e);
      this.module = null;
    }
    return this.module;
  }
}

/** A per-document engine session; JSON at the boundary, typed inside. */
export class EngineSession {
  constructor(private session: WasmSession | null) {}

  key(key: string): Effects | null {
    return this.parse(() => this.session?.key(key));
  }

  /**
   * A key from a register being run: `key`, except that a recording in
   * progress leaves it out, as Vim records what is typed and not what a
   * register replays.
   */
  feed(key: string): Effects | null {
    return this.parse(() => this.session?.feed(key));
  }

  /**
   * Text a register being run types in insert mode, put in at every cursor
   * as the engine's own edit — not through the editor's typing, whose
   * auto-indent and auto-closing already shaped the text when it was
   * recorded.
   */
  insertText(text: string): Effects | null {
    return this.parse(() => this.session?.insert_text(text));
  }

  reset(text: string, line: number, col: number): void {
    try {
      this.session?.reset(text, line, col);
    } catch (e) {
      console.error('vim-engine reset failed:', e);
    }
  }

  applyChanges(changes: EngineChange[]): void {
    try {
      this.session?.apply_changes(JSON.stringify(changes));
    } catch (e) {
      console.error('vim-engine apply_changes failed:', e);
    }
  }

  setPosition(line: number, col: number): Effects | null {
    return this.parse(() => this.session?.set_position(line, col));
  }

  setSelection(anchor: EnginePos, active: EnginePos, byHand = false): Effects | null {
    return this.parse(() =>
      this.session?.set_selection(
        anchor.line,
        anchor.col,
        active.line,
        active.col,
        byHand,
      ),
    );
  }

  /**
   * The editor's whole selection set, primary first. One selection behaves
   * like `setPosition`/`setSelection`; more put the engine in multi-cursor
   * editing, where every key runs at every cursor.
   *
   * `byHand` says the user drew the selection (pointer drag, shift+arrow)
   * rather than a command leaving it behind; only the former starts visual
   * mode. Defaults to false: a cursor sync the controller does on its own
   * account is nobody's drag.
   */
  setCursors(selections: readonly EngineSelection[], byHand = false): Effects | null {
    const wire = selections.map((s) => ({
      anchorLine: s.anchor.line,
      anchorCol: s.anchor.col,
      activeLine: s.active.line,
      activeCol: s.active.col,
    }));
    return this.parse(() => this.session?.set_cursors(JSON.stringify(wire), byHand));
  }

  /**
   * The inclusive line span the editor has on screen. EasyMotion only labels
   * what the user can see, so this is reported before the keys that could
   * open a jump.
   */
  setView(first: number, last: number): void {
    try {
      this.session?.set_view(first, last);
    } catch (e) {
      console.error('vim-engine set_view failed:', e);
    }
  }

  /**
   * EasyMotion settings: whether jumps are on, the key sequence that opens
   * one (written as a Vim mapping — `<space><space>`, `,,`) and the
   * characters its labels are spelled with, in the order they are handed out.
   */
  setEasyMotion(enabled: boolean, trigger: string, keys: string): void {
    try {
      this.session?.set_easy_motion(enabled, trigger, keys);
    } catch (e) {
      console.error('vim-engine set_easy_motion failed:', e);
    }
  }

  /**
   * The document this session mirrors, by URI: file marks (`mA`) remember
   * it, and `'A` from another document hands it back in a `gotoMark`.
   */
  setDocument(uri: string): void {
    try {
      this.session?.set_document(uri);
    } catch (e) {
      console.error('vim-engine set_document failed:', e);
    }
  }

  /**
   * Whether `key` would read the system clipboard (`"+p`, `:pu +`). The
   * engine runs a key to completion while the clipboard API is async, so the
   * host asks before every key, and when the answer is yes reads the
   * clipboard and hands it over with `setClipboard` before sending the key.
   */
  wantsClipboard(key: string): boolean {
    try {
      return this.session?.wants_clipboard(key) ?? false;
    } catch {
      return false;
    }
  }

  /** The system clipboard's text, as just read for a key that wants it. */
  setClipboard(text: string): void {
    try {
      this.session?.set_clipboard(text);
    } catch (e) {
      console.error('vim-engine set_clipboard failed:', e);
    }
  }

  /** What register `name` holds; `null` when it is empty. */
  register(name: string): EngineRegister | null {
    try {
      const json = this.session?.register(name);
      return json ? (JSON.parse(json) as EngineRegister | null) : null;
    } catch {
      return null;
    }
  }

  /**
   * Write register `name` as a recording does: uppercase appends, `""`
   * stays where it pointed. False for one that cannot be written.
   */
  setRegister(name: string, text: string, linewise: boolean): boolean {
    try {
      return this.session?.set_register(name, text, linewise) ?? false;
    } catch {
      return false;
    }
  }

  mode(): EngineMode {
    try {
      return (this.session?.mode() as EngineMode) ?? 'normal';
    } catch {
      return 'normal';
    }
  }

  text(): string | null {
    try {
      return this.session?.text() ?? null;
    } catch {
      return null;
    }
  }

  dispose(): void {
    try {
      this.session?.free();
    } catch {
      // free() throwing means the wasm instance is already gone.
    }
    this.session = null;
  }

  private parse(call: () => string | undefined): Effects | null {
    try {
      const json = call();
      if (!json) return null;
      const wire = JSON.parse(json) as WireEffects | null;
      if (!wire) return null;
      const { editCount, ...fx } = wire;
      return { ...fx, edits: editCount > 0 ? this.takeEdits() : [] };
    } catch (e) {
      console.error('vim-engine call failed:', e);
      return null;
    }
  }

  /**
   * Decode the binary edit block (wasm_api's `take_edits`): a u32 count,
   * count×5 u32 header rows, then every edit's text as one UTF-8 blob. One
   * decode plus string slices — JSON-escaping a `:%s` over a big file costs
   * milliseconds on both sides of the boundary; this doesn't.
   */
  private takeEdits(): EngineEdit[] {
    if (!this.session) return [];
    const buf = this.session.take_edits();
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    const count = view.getUint32(0, true);
    const blob = utf8.decode(buf.subarray(4 + count * 20));
    const edits: EngineEdit[] = new Array(count);
    let at = 0; // UTF-16 offset into the blob, like the header lengths
    for (let i = 0; i < count; i++) {
      const o = 4 + i * 20;
      const len = view.getUint32(o + 16, true);
      edits[i] = {
        start: { line: view.getUint32(o, true), col: view.getUint32(o + 4, true) },
        end: { line: view.getUint32(o + 8, true), col: view.getUint32(o + 12, true) },
        text: blob.slice(at, at + len),
      };
      at += len;
    }
    return edits;
  }
}

const utf8 = new TextDecoder();
