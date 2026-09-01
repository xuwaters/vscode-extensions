// RFC 012 §8's wasm budget, measured against the artifact that actually ships.
//
// A full reparse and reanalysis of a 1,000-line shader gets **25 ms** in wasm.
// The server has no incremental parsing — the CST and the analysis are rebuilt
// on every keystroke — so this is the number that decides whether typing in a
// large shader feels alive.
//
// The test reads `wasm/wgsl_lsp_wasm.js`, which `pnpm build:wasm` produces and
// which is not committed, and **skips** when it is absent: the same pattern the
// Rust corpus tests use for `temp/`. Timing is the best of thirty runs, and the
// assertion is the budget itself, which there is comfortable room under.

import { createRequire } from 'node:module';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// Resolved from the working directory rather than from `import.meta`, which
// the CommonJS build this package emits does not allow. Vitest runs with the
// package root as its cwd.
const WASM = resolve(process.cwd(), 'wasm/wgsl_lsp_wasm.js');

/** RFC 012 §8. */
const BUDGET_MS = 25;

/**
 * A shader of roughly `lines` lines that exercises every layer: macros and a
 * conditional for the preprocessor, structs and interface blocks for the
 * parser, and enough expressions to make the analyzer work for its answer.
 *
 * The same fixture the native measurement uses
 * (`crates/wgsl-shader/wgsl-lsp-core/tests/budgets.rs`), so the two numbers
 * are comparable.
 */
function shader(lines: number): string {
  let source =
    '#version 450\n#define SCALE(x) ((x) * 2.0)\n#define AMBIENT 0.1\n' +
    'layout(set = 0, binding = 0) uniform Camera {\n    mat4 view;\n    mat4 proj;\n} camera;\n' +
    'layout(location = 0) in vec3 v_normal;\n' +
    'layout(location = 1) in vec2 v_uv;\n' +
    'layout(location = 0) out vec4 out_colour;\n' +
    'layout(set = 0, binding = 1) uniform texture2D albedo;\n' +
    'layout(set = 0, binding = 2) uniform sampler albedo_sampler;\n' +
    'struct Light {\n    vec3 colour;\n    float intensity;\n};\n';

  let written = source.split('\n').length;
  let index = 0;
  while (written < lines - 12) {
    source +=
      `float helper${index}(vec3 normal, float falloff) {\n` +
      `    float lambert${index} = max(dot(normalize(normal), vec3(0.0, 1.0, 0.0)), 0.0);\n` +
      `    vec3 tinted${index} = normal * SCALE(lambert${index}) + vec3(AMBIENT);\n` +
      `    return clamp(tinted${index}.x / falloff, 0.0, 1.0);\n}\n`;
    written += 5;
    index += 1;
  }

  source += '#ifdef NEVER\nfloat dead() { return 0.0; }\n#endif\n';
  source += 'void main() {\n    float total = AMBIENT;\n';
  for (let i = 0; i < Math.min(index, 8); i++) {
    source += `    total += helper${i}(v_normal, 2.0);\n`;
  }
  source +=
    '    vec4 base = texture(sampler2D(albedo, albedo_sampler), v_uv);\n' +
    '    out_colour = vec4(base.rgb * total, base.a);\n}\n';
  return source;
}

describe.skipIf(!existsSync(WASM))('the wasm performance budget', () => {
  it('reparses and reanalyses a 1,000-line shader inside 25 ms', () => {
    const require = createRequire(`${WASM}`);
    const { ShaderServer } = require(WASM) as {
      ShaderServer: new (options: unknown) => {
        onNotification(method: string, params: unknown): void;
        drainEvents(): unknown[];
      };
    };

    const source = shader(1_000);
    expect(source.split('\n').length).toBeGreaterThan(950);

    const server = new ShaderServer({ settings: {} });
    const uri = 'file:///shaders/big.frag';
    server.onNotification('textDocument/didOpen', {
      textDocument: { uri, languageId: 'glsl', version: 1, text: source },
    });
    server.drainEvents();

    let best = Infinity;
    for (let n = 0; n < 30; n++) {
      const edited = source.replace('float total = AMBIENT;', `float total = ${n}.0;`);
      const start = process.hrtime.bigint();
      server.onNotification('textDocument/didChange', {
        textDocument: { uri, version: n + 2 },
        contentChanges: [{ text: edited }],
      });
      // `didChange` only reparses; asking for validation is what forces the
      // analysis, which is the half the budget is really about.
      server.onNotification('wgsl/validate', { textDocument: { uri } });
      server.drainEvents();
      best = Math.min(best, Number(process.hrtime.bigint() - start) / 1e6);
    }

    // eslint-disable-next-line no-console
    console.log(`P5-10 wasm: best of 30 = ${best.toFixed(2)} ms (budget ${BUDGET_MS} ms)`);
    expect(best).toBeLessThanOrEqual(BUDGET_MS);
  });
});
