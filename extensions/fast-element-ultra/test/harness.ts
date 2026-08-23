/**
 * Test harness: a real `ts.LanguageService` over in-memory fixture files (or
 * real corpus files), the real WASM engine, and the plugin's own code paths —
 * no tsserver, no mocks. Fixtures resolve `@microsoft/fast-element` to the
 * genuine installed package, so decorators and directive types are the real
 * ones.
 */

import * as fs from 'node:fs';
import { createRequire } from 'node:module';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

import * as ts from 'typescript';

import { SafeEngine } from '../tsplugin/engine.js';
import { Logger } from '../tsplugin/logger.js';
import type { PluginSettings } from '../tsplugin/config.js';
import { decorateLanguageService, FastService } from '../tsplugin/service.js';
import { DIAGNOSTIC_SOURCE } from '../tsplugin/protocol.js';

const here = path.dirname(fileURLToPath(import.meta.url));
export const extensionRoot = path.join(here, '..');
export const repoRoot = path.join(extensionRoot, '..', '..');

const require0 = createRequire(import.meta.url);

export const wasmGluePath = path.join(extensionRoot, 'wasm', 'fast_analyzer_wasm.js');

/** CI's node job runs without wasm-pack; suites skip themselves when the
 * artifact is absent, the same arrangement as typst-ultra's. */
export const wasmBuilt = fs.existsSync(wasmGluePath);

// One compiled WASM module for the whole test run; instances are cheap.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let wasmModule: any;
function loadWasm(): unknown {
  // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
  wasmModule ??= require0(wasmGluePath);
  return wasmModule;
}

/** Virtual fixture files live under this never-written directory, so module
 * resolution walks up into the extension's real node_modules. */
export const FIXTURE_ROOT = path.join(extensionRoot, '__fixtures__');

export interface Harness {
  ls: ts.LanguageService;
  decorated: ts.LanguageService;
  service: FastService;
  engine: SafeEngine;
  logger: Logger;
  logLines: string[];
  updateFile(fileName: string, content: string): void;
  /** Only our diagnostics, sorted by position. */
  fastDiagnostics(fileName: string): ts.Diagnostic[];
  setSettings(settings: PluginSettings): void;
}

const DEFAULT_OPTIONS: ts.CompilerOptions = {
  strict: true,
  target: ts.ScriptTarget.ES2022,
  module: ts.ModuleKind.ES2022,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  experimentalDecorators: true,
  useDefineForClassFields: false,
  lib: ['lib.es2022.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'],
  resolveJsonModule: true,
  skipLibCheck: true,
  noEmit: true,
};

export function createHarness(
  files: Record<string, string>,
  options?: {
    settings?: PluginSettings;
    rootFiles?: string[];
    compilerOptions?: ts.CompilerOptions;
  },
): Harness {
  const virtual = new Map<string, { content: string; version: number }>();
  for (const [name, content] of Object.entries(files)) {
    virtual.set(normalize(name), { content, version: 1 });
  }
  let settings: PluginSettings = options?.settings ?? { strict: true, logging: 'off' };

  const compilerOptions = options?.compilerOptions ?? DEFAULT_OPTIONS;
  const rootFiles = options?.rootFiles ?? [...virtual.keys()];

  const host: ts.LanguageServiceHost = {
    getScriptFileNames: () => rootFiles,
    getScriptVersion: (fileName) =>
      String(virtual.get(normalize(fileName))?.version ?? 1),
    getScriptSnapshot: (fileName) => {
      const entry = virtual.get(normalize(fileName));
      if (entry) return ts.ScriptSnapshot.fromString(entry.content);
      const onDisk = ts.sys.readFile(fileName);
      return onDisk === undefined ? undefined : ts.ScriptSnapshot.fromString(onDisk);
    },
    getCurrentDirectory: () => extensionRoot,
    getCompilationSettings: () => compilerOptions,
    getDefaultLibFileName: (opts) => ts.getDefaultLibFilePath(opts),
    fileExists: (fileName) => virtual.has(normalize(fileName)) || ts.sys.fileExists(fileName),
    readFile: (fileName) =>
      virtual.get(normalize(fileName))?.content ?? ts.sys.readFile(fileName),
    readDirectory: ts.sys.readDirectory,
    directoryExists: (dir) =>
      [...virtual.keys()].some((f) => f.startsWith(`${normalize(dir)}/`)) ||
      ts.sys.directoryExists(dir),
    getDirectories: ts.sys.getDirectories,
  };

  const ls = ts.createLanguageService(host);
  const logLines: string[] = [];
  const logger = new Logger((message) => logLines.push(message));
  logger.level = 'error';
  const engine = new SafeEngine(logger, loadWasm() as never);
  const service = new FastService({
    ts,
    languageService: ls,
    languageServiceHost: host,
    engine,
    logger,
    getSettings: () => settings,
    projectRoot: extensionRoot,
  });
  const decorated = decorateLanguageService(service, ls, logger);

  return {
    ls,
    decorated,
    service,
    engine,
    logger,
    logLines,
    updateFile(fileName, content) {
      const key = normalize(fileName);
      const previous = virtual.get(key);
      virtual.set(key, { content, version: (previous?.version ?? 1) + 1 });
    },
    fastDiagnostics(fileName) {
      return decorated
        .getSemanticDiagnostics(fileName)
        .filter((d) => d.source === DIAGNOSTIC_SOURCE)
        .sort((a, b) => (a.start ?? 0) - (b.start ?? 0));
    },
    setSettings(next) {
      settings = next;
      service.onSettingsChanged();
    },
  };
}

export function fixture(name: string): string {
  return normalize(path.join(FIXTURE_ROOT, name));
}

function normalize(fileName: string): string {
  return fileName.replace(/\\/g, '/');
}

export function messages(diagnostics: readonly ts.Diagnostic[]): string[] {
  return diagnostics.map((d) => ts.flattenDiagnosticMessageText(d.messageText, ' '));
}

export function codes(diagnostics: readonly ts.Diagnostic[]): number[] {
  return diagnostics.map((d) => d.code);
}
