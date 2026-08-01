import * as fs from 'fs';
import * as path from 'path';
import type { Frontmatter, Patch, TocEntry } from './messages';

/** Options accepted by the engine (crates/markdown-engine `RenderOptions`). */
export interface EngineOptions {
  breaks: boolean;
  linkify: boolean;
  typographer: boolean;
  html: boolean;
  math: boolean;
  mermaid: boolean;
  alerts: boolean;
  emoji: boolean;
  wikilinks: boolean;
}

/** Engine `RenderResult` (JSON-parsed). */
export interface EngineRenderResult {
  seq: number;
  reset: boolean;
  patches: Patch[];
  toc: TocEntry[];
  frontmatter: Frontmatter | null;
  stats: { blockCount: number };
}

/** Shape of the wasm-bindgen `Session` handle. */
interface WasmSession {
  render(markdown: string, optionsJson: string): string;
  free(): void;
}

interface WasmModule {
  Session: new () => WasmSession;
}

/**
 * Loads the wasm-pack-built `markdown_engine` module lazily (on first preview
 * open, not activation) and hands out per-document render sessions. Degrades
 * gracefully when `wasm/` is missing — typical when the extension is run
 * before `pnpm run build:wasm` finishes.
 */
export class EngineBridge {
  private module: WasmModule | null | undefined;

  constructor(private readonly extensionPath: string) {}

  get ready(): boolean {
    return this.loadModule() !== null;
  }

  createSession(): EngineSession | null {
    const mod = this.loadModule();
    return mod ? new EngineSession(new mod.Session()) : null;
  }

  private loadModule(): WasmModule | null {
    if (this.module !== undefined) return this.module;
    const entry = path.join(this.extensionPath, 'wasm', 'markdown_engine.js');
    if (!fs.existsSync(entry)) {
      console.warn(
        'markdown-engine WASM bundle not found. Run `pnpm run build:wasm` in extensions/markdown-live-preview.',
      );
      this.module = null;
      return null;
    }
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      this.module = require(entry) as WasmModule;
    } catch (e) {
      console.error('Failed to load markdown-engine WASM:', e);
      this.module = null;
    }
    return this.module;
  }
}

/**
 * A per-document engine session; retains previous block hashes WASM-side so
 * each render returns a minimal patch script.
 */
export class EngineSession {
  constructor(private session: WasmSession | null) {}

  /** Render; returns null if the session is disposed or the engine failed. */
  render(markdown: string, options: EngineOptions): EngineRenderResult | null {
    if (!this.session) return null;
    try {
      const json = this.session.render(markdown, JSON.stringify(options));
      return JSON.parse(json) as EngineRenderResult;
    } catch (e) {
      console.error('markdown-engine render failed:', e);
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
}
