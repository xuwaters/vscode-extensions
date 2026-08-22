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
  | { kind: 'indentLines'; startLine: number; endLine: number; dedent: boolean }
  | { kind: 'scroll'; to: 'center' | 'top' | 'bottom' }
  | { kind: 'openLine'; above: boolean }
  | { kind: 'showHover' };

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

/** One engine response (crates/vim-engine `Effects`). */
export interface Effects {
  mode: EngineMode;
  selections: EngineSelection[];
  edits: EngineEdit[];
  commands: EngineCommand[];
  pending: string;
  /** Status-bar report: `:s` counts, ex errors, a search that found nothing. */
  message?: string;
  /** Search-typing UI state; absent outside a `/`/`?` prompt's lifetime. */
  search?: EngineSearchUi;
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
