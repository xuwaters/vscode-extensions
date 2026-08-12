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
  | { kind: 'scroll'; to: 'center' | 'top' | 'bottom' };

/** One engine response (crates/vim-engine `Effects`). */
export interface Effects {
  mode: EngineMode;
  selections: EngineSelection[];
  edits: EngineEdit[];
  commands: EngineCommand[];
  pending: string;
  /** Status-bar report: `:s` counts, ex errors, a search that found nothing. */
  message?: string;
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
  ): string;
  mode(): string;
  text(): string;
  free(): void;
}

interface WasmModule {
  Session: new (text: string, line: number, col: number) => WasmSession;
}

/**
 * Loads the wasm-pack-built `vim_engine` module lazily and hands out
 * per-document sessions. Degrades gracefully when `wasm/` is missing —
 * typical when the extension runs before `pnpm run build:wasm`.
 */
export class EngineBridge {
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

  setSelection(anchor: EnginePos, active: EnginePos): Effects | null {
    return this.parse(() =>
      this.session?.set_selection(anchor.line, anchor.col, active.line, active.col),
    );
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
      return JSON.parse(json) as Effects | null;
    } catch (e) {
      console.error('vim-engine call failed:', e);
      return null;
    }
  }
}
