/**
 * P1-07 and P1-10: containment tested by deliberately panicking the real
 * artifact, and an end-to-end pass through the assembled plugin. On
 * wasm32-unknown-unknown a Rust panic is a trap that surfaces as a JS
 * RuntimeError — the SafeEngine wrapper is the containment layer, and after
 * it poisons the engine, TypeScript's own features keep working.
 */

import { createRequire } from 'node:module';
import * as path from 'node:path';

import { describe, expect, it } from 'vitest';

import { SafeEngine } from '../tsplugin/engine.js';
import { Logger } from '../tsplugin/logger.js';
import { createHarness, extensionRoot, fixture, wasmBuilt, wasmGluePath } from './harness.js';

const require0 = createRequire(import.meta.url);
// eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
const wasmModule = wasmBuilt ? require0(wasmGluePath) : undefined;

describe.skipIf(!wasmBuilt)('panic containment (P1-07)', () => {
  it('a panic is caught, a second poisons the engine, and TypeScript survives', () => {
    const FILE = fixture('survives.ts');
    const harness = createHarness({
      [FILE]: `
        import { FASTElement, customElement, html } from '@microsoft/fast-element';
        @customElement('ok-el')
        export class OkEl extends FASTElement {}
        export const t = html<OkEl>\`<div><butto>x</div>\`;
        export const brokenTypescript: number = 'not a number';
      `,
    });
    // Working before the panic: our diagnostic is present.
    expect(harness.fastDiagnostics(FILE).length).toBeGreaterThan(0);

    harness.engine.debugPanic();
    expect(harness.engine.state).toBe('ok'); // one strike
    harness.engine.debugPanic();
    expect(harness.engine.state).toBe('poisoned'); // two strikes

    // Our diagnostics are gone; TypeScript's own are not.
    const after = harness.decorated.getSemanticDiagnostics(FILE);
    expect(after.filter((d) => d.source === 'fast-element-ultra')).toEqual([]);
    expect(after.some((d) => d.code === 2322)).toBe(true); // TS2322 not assignable
    // And every decorated method still answers through the fallback.
    expect(() => harness.decorated.getCompletionsAtPosition(FILE, 10, undefined)).not.toThrow();
    // The failure is in the log, not swallowed.
    expect(harness.logLines.some((l) => l.includes('poisoned'))).toBe(true);
  });

  it('bad payloads are engine errors, not poison', () => {
    const logger = new Logger(() => {});
    const engine = new SafeEngine(logger, wasmModule);
    // Engine-level errors (a payload serde rejects) return false and never
    // count as strikes toward poisoning.
    for (let i = 0; i < 5; i++) {
      expect(engine.setConfig({ rules: 5 } as never)).toBe(false);
    }
    expect(engine.state).toBe('ok');
  });
});

describe.skipIf(!wasmBuilt)('the assembled plugin (P1-10)', () => {
  it('node_modules/wx-fast-element-tsplugin is a loadable tsserver plugin', () => {
    const pluginDir = path.join(extensionRoot, 'node_modules', 'wx-fast-element-tsplugin');
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    const factory = require0(pluginDir);
    expect(typeof factory).toBe('function');
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    const ts = require0('typescript');
    const pluginModule = factory({ typescript: ts });
    expect(typeof pluginModule.create).toBe('function');
    expect(typeof pluginModule.onConfigurationChanged).toBe('function');
    // The wasm ships next to it and loads.
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    const wasm = require0(path.join(pluginDir, 'fast_analyzer_wasm.js'));
    const engine = new wasm.Engine();
    expect(engine.setConfig('{}')).toBe(true);
  });

  it('the version gate refuses an unsupported TypeScript and returns the LS untouched', () => {
    const pluginDir = path.join(extensionRoot, 'node_modules', 'wx-fast-element-tsplugin');
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    const factory = require0(pluginDir);
    const fakeTs = { versionMajorMinor: '4.9', version: '4.9.5' };
    const pluginModule = factory({ typescript: fakeTs });
    const languageService = { marker: true };
    const logged: string[] = [];
    const created = pluginModule.create({
      languageService,
      languageServiceHost: {},
      config: {},
      project: {
        projectService: { logger: { info: (m: string) => logged.push(m) } },
        getCurrentDirectory: () => '/',
        getProjectName: () => 'test',
      },
    });
    expect(created).toBe(languageService);
    // The gate does not log unless logging is on — but a refused version is
    // an error-level event, so with logging left off it stays quiet by
    // default and the LS passes through untouched. Turn logging on:
    const created2 = pluginModule.create({
      languageService,
      languageServiceHost: {},
      config: { logging: 'error' },
      project: {
        projectService: { logger: { info: (m: string) => logged.push(m) } },
        getCurrentDirectory: () => '/',
        getProjectName: () => 'test',
      },
    });
    expect(created2).toBe(languageService);
    expect(logged.some((l) => l.includes('outside the supported range'))).toBe(true);
  });
});
