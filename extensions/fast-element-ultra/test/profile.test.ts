/**
 * Where the plugin's time goes on a real project. Opt-in:
 *
 *   FAST_PROFILE_PROJECT=/path/to/a/ts/project pnpm vitest run test/profile.test.ts
 *
 * The project's own `tsconfig.json` supplies the compiler options and root
 * files, so the program is the one tsserver would build. What is timed is the
 * plugin's own work: the first `sync()`, the tag-name-map scan inside it, and
 * — the number that decides whether typing is comfortable — the cost of a
 * `getSemanticDiagnostics` after an edit, against TypeScript's own cost for
 * the same edit.
 *
 * Results print as a block; nothing is asserted beyond the run completing,
 * because the numbers are the point.
 */

import * as fs from 'node:fs';
import * as path from 'node:path';

import * as ts from 'typescript';
import { describe, expect, it } from 'vitest';

import { extractFile, Interner } from '../tsplugin/extract.js';
import { createHarness, wasmBuilt } from './harness.js';

const projectPath = process.env.FAST_PROFILE_PROJECT;
const enabled = projectPath !== undefined && wasmBuilt;

describe.runIf(enabled)('profile a real project', () => {
  it('times sync, tag-name-map discovery and the per-edit overhead', () => {
    const root = path.resolve(projectPath!);
    const configPath = ts.findConfigFile(root, ts.sys.fileExists, 'tsconfig.json');
    expect(configPath, `no tsconfig.json under ${root}`).toBeDefined();
    const parsed = ts.parseJsonConfigFileContent(
      ts.readConfigFile(configPath!, ts.sys.readFile).config,
      ts.sys,
      path.dirname(configPath!),
    );

    const lines: string[] = [];
    const harness = createHarness(
      {},
      {
        rootFiles: parsed.fileNames,
        compilerOptions: parsed.options,
        settings: { logging: 'off' },
      },
    );

    // -- the program itself, for scale ------------------------------------
    const p0 = process.hrtime.bigint();
    const program = harness.ls.getProgram()!;
    const p1 = process.hrtime.bigint();
    const all = program.getSourceFiles();
    const slice = all.filter(
      (f) =>
        !f.isDeclarationFile &&
        !f.fileName.includes('/node_modules/') &&
        f.text.includes('fast-element'),
    );
    lines.push(`root files: ${parsed.fileNames.length}`);
    lines.push(`program: ${all.length} source files, built in ${ms(p0, p1)} ms`);
    lines.push(`FAST slice (re-extracted on every new program): ${slice.length} files`);

    // -- first sync: extraction of the slice + tag-name-map scan ----------
    const s0 = process.hrtime.bigint();
    harness.service.sync();
    const s1 = process.hrtime.bigint();
    lines.push(`cold sync(): ${ms(s0, s1)} ms`);

    // -- the tag-name-map scan on its own ---------------------------------
    const svc = harness.service as unknown as {
      ambientKey: string | undefined;
      ambientComponents: unknown[];
      files: Map<string, unknown>;
      outsideVersions: Map<string, string>;
      syncAmbient?(program: ts.Program, checker: ts.TypeChecker): void;
    };
    if (typeof svc.syncAmbient === 'function') {
      lines.push(`tag-name-map components found: ${svc.ambientComponents.length}`);
      const checker = program.getTypeChecker();
      const a0 = process.hrtime.bigint();
      svc.ambientKey = undefined;
      svc.syncAmbient(program, checker);
      const a1 = process.hrtime.bigint();
      lines.push(`syncAmbient() alone, checker warm: ${ms(a0, a1)} ms`);
    } else {
      lines.push('tag-name-map discovery: not in this build');
    }

    // -- a sync with the same program: the cheap path ---------------------
    const w0 = process.hrtime.bigint();
    for (let i = 0; i < 10; i++) harness.service.sync();
    const w1 = process.hrtime.bigint();
    lines.push(
      `sync() with an unchanged program: ${(Number(w1 - w0) / 1e6 / 10).toFixed(2)} ms each`,
    );

    // -- a sync with every cache dropped: the whole slice at once ---------
    // What every keystroke used to cost, kept as the ceiling to measure the
    // incremental path against.
    const times: number[] = [];
    for (let i = 0; i < 3; i++) {
      svc.files.clear();
      svc.outsideVersions.clear();
      svc.ambientKey = undefined;
      const r0 = process.hrtime.bigint();
      harness.service.sync();
      const r1 = process.hrtime.bigint();
      times.push(Number(r1 - r0) / 1e6);
    }
    lines.push(`sync() of the whole slice: ${times.map((t) => t.toFixed(1)).join(', ')} ms`);

    // -- what a keystroke really costs, end to end ------------------------
    // A real edit gives the plugin a *new* Program with a *cold* checker, so
    // the whole slice is re-extracted at full price. Against TypeScript's own
    // cost for the same edit, the difference is the plugin's.
    const target = slice.find((f) => f.text.includes('html`')) ?? slice[0];
    if (target) {
      const before = fs.readFileSync(target.fileName, 'utf8');
      const bare: number[] = [];
      const decorated: number[] = [];
      for (let i = 0; i < 6; i++) {
        harness.updateFile(target.fileName, `${before}\n// edit ${i}\n`);
        const b0 = process.hrtime.bigint();
        harness.ls.getSemanticDiagnostics(target.fileName);
        const b1 = process.hrtime.bigint();
        bare.push(Number(b1 - b0) / 1e6);

        harness.updateFile(target.fileName, `${before}\n// edit ${i} ${i}\n`);
        const d0 = process.hrtime.bigint();
        harness.decorated.getSemanticDiagnostics(target.fileName);
        const d1 = process.hrtime.bigint();
        decorated.push(Number(d1 - d0) / 1e6);
      }
      lines.push(`per edit, TypeScript alone:  ${bare.map((t) => t.toFixed(0)).join(', ')} ms`);
      lines.push(`per edit, with the plugin:   ${decorated.map((t) => t.toFixed(0)).join(', ')} ms`);
      lines.push(
        `plugin overhead per edit: ${(median(decorated) - median(bare)).toFixed(0)} ms (median)`,
      );
    }

    // -- last, because it builds a second program: extraction vs engine ---
    // Which half of a re-sync is expensive. A fresh program means a checker
    // as cold as the one tsserver hands over after an edit.
    {
      const fresh = ts.createProgram({ rootNames: parsed.fileNames, options: parsed.options });
      const freshChecker = fresh.getTypeChecker();
      const interner = new Interner();
      let extractMs = 0;
      let upsertMs = 0;
      let templates = 0;
      let components = 0;
      for (const file of slice) {
        const sourceFile = fresh.getSourceFile(file.fileName);
        if (!sourceFile) continue;
        const x0 = process.hrtime.bigint();
        const extraction = extractFile({
          ts,
          checker: freshChecker,
          sourceFile,
          htmlTemplateTags: ['html'],
          cssTemplateTags: ['css'],
          interner,
          resolveModule: (specifier, fromFile) => {
            const resolved = ts.resolveModuleName(
              specifier,
              fromFile,
              parsed.options,
              ts.sys,
            ).resolvedModule;
            return resolved
              ? {
                  resolvedFileName: resolved.resolvedFileName,
                  isExternal: resolved.isExternalLibraryImport === true,
                }
              : undefined;
          },
        });
        const x1 = process.hrtime.bigint();
        harness.engine.upsertFile(extraction.upsert);
        const x2 = process.hrtime.bigint();
        extractMs += Number(x1 - x0) / 1e6;
        upsertMs += Number(x2 - x1) / 1e6;
        templates += extraction.templates.length;
        components += extraction.upsert.components.length;
      }
      lines.push(
        `extraction of the slice, cold checker: ${extractMs.toFixed(0)} ms for ${templates} templates / ${components} components`,
      );
      lines.push(`engine upsertFile for the same: ${upsertMs.toFixed(0)} ms`);
    }

    console.log(`\n${lines.join('\n')}\n`);
    expect(lines.length).toBeGreaterThan(0);
  }, 600_000);
});

function ms(a: bigint, b: bigint): string {
  return (Number(b - a) / 1e6).toFixed(2);
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? 0;
}
