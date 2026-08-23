/**
 * Loads the WASM engine and wraps every call in the containment the design
 * demands (architecture.md §1.1): on `wasm32-unknown-unknown` a Rust panic is
 * a trap that surfaces here as a JS `RuntimeError`, so the try/catch in this
 * file — not `catch_unwind` — is the layer that keeps tsserver alive. Two
 * throws poison the engine; a poisoned engine answers nothing, and the
 * decorated language service falls through to plain TypeScript.
 */

import * as path from 'path';

import type {
  AnalyzeResult,
  EngineConfig,
  FileDiagnostic,
  UpsertFile,
} from './protocol.js';
import type { Logger } from './logger.js';

interface WasmEngine {
  setConfig(json: string): boolean;
  upsertFile(json: string): boolean;
  removeFile(fileName: string): boolean;
  analyze(documentId: string): string | undefined;
  query(json: string): string | undefined;
  lastError(): string | undefined;
  debugPanic(): string | undefined;
}

interface WasmModule {
  Engine: new () => WasmEngine;
  engineVersion(): string;
}

/**
 * Locate the wasm glue: next to the assembled plugin inside the VSIX, or in
 * the extension's `wasm/` directory when running from the repo (tests, F5).
 */
export function loadWasmModule(require0: NodeJS.Require = require): WasmModule {
  const candidates = [
    path.join(__dirname, 'fast_analyzer_wasm.js'),
    path.join(__dirname, '..', 'wasm', 'fast_analyzer_wasm.js'),
    path.join(__dirname, '..', '..', 'wasm', 'fast_analyzer_wasm.js'),
  ];
  let lastError: unknown;
  for (const candidate of candidates) {
    try {
      return require0(candidate) as WasmModule;
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError instanceof Error ? lastError : new Error(String(lastError));
}

export type EngineState = 'ok' | 'poisoned' | 'unavailable';

export class SafeEngine {
  private inner: WasmEngine | undefined;
  private failures = 0;
  state: EngineState = 'ok';

  constructor(
    private readonly logger: Logger,
    wasm?: WasmModule,
  ) {
    try {
      const module = wasm ?? loadWasmModule();
      this.inner = new module.Engine();
    } catch (error) {
      this.state = 'unavailable';
      this.logger.error(`engine failed to load: ${describe(error)}`);
    }
  }

  get available(): boolean {
    return this.state === 'ok' && this.inner !== undefined;
  }

  private guard<T>(context: string, body: (engine: WasmEngine) => T): T | undefined {
    if (!this.available || !this.inner) return undefined;
    try {
      return body(this.inner);
    } catch (error) {
      this.failures += 1;
      this.logger.error(`engine threw in ${context}: ${describe(error)}`);
      if (this.failures >= 2) {
        // A trapped instance's memory is not trustworthy; stop asking it
        // anything. TypeScript's own features are unaffected.
        this.state = 'poisoned';
        this.inner = undefined;
        this.logger.error('engine poisoned after repeated failures; falling through to plain TypeScript');
      }
      return undefined;
    }
  }

  /** Log-and-clear the engine-level error behind a false/undefined result. */
  private drainError(context: string): void {
    const message = this.guard(context, (e) => e.lastError());
    if (message) this.logger.error(`${context}: ${message}`);
  }

  setConfig(config: EngineConfig): boolean {
    const ok = this.guard('setConfig', (e) => e.setConfig(JSON.stringify(config))) ?? false;
    if (!ok) this.drainError('setConfig');
    return ok;
  }

  upsertFile(upsert: UpsertFile): boolean {
    const ok = this.guard('upsertFile', (e) => e.upsertFile(JSON.stringify(upsert))) ?? false;
    if (!ok) this.drainError('upsertFile');
    return ok;
  }

  removeFile(fileName: string): void {
    this.guard('removeFile', (e) => e.removeFile(fileName));
  }

  analyze(documentId: string): AnalyzeResult | undefined {
    const raw = this.guard('analyze', (e) => e.analyze(documentId));
    if (raw === undefined) {
      this.drainError('analyze');
      return undefined;
    }
    return JSON.parse(raw) as AnalyzeResult;
  }

  query<T>(request: Record<string, unknown>): T | undefined {
    const raw = this.guard('query', (e) => e.query(JSON.stringify(request)));
    if (raw === undefined) {
      this.drainError('query');
      return undefined;
    }
    const parsed = JSON.parse(raw) as T | null;
    return parsed === null ? undefined : parsed;
  }

  severities(): Record<string, string> {
    return this.query<Record<string, string>>({ type: 'severities' }) ?? {};
  }

  fileDiagnostics(fileName: string): FileDiagnostic[] {
    return this.query<FileDiagnostic[]>({ type: 'fileDiagnostics', fileName }) ?? [];
  }

  /** Test hook: trip the containment path against the real artifact. */
  debugPanic(): void {
    this.guard('debugPanic', (e) => e.debugPanic());
  }
}

function describe(error: unknown): string {
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}
