// Outline extraction for both shader languages, done line by line so a file
// that does not parse still gets an outline. Kept free of the `vscode` module so
// it can be unit tested; extension.ts maps the result onto DocumentSymbols.

export type ShaderSymbolKind = 'function' | 'struct' | 'variable';

export interface ShaderSymbol {
  name: string;
  kind: ShaderSymbolKind;
  /** Zero-based line the declaration starts on. */
  line: number;
}

// ── WGSL ───────────────────────────────────────────────────────────

const WGSL_FN = /\bfn\s+([A-Za-z0-9_]+)\s*\(/;
const WGSL_STRUCT = /\bstruct\s+([A-Za-z0-9_]+)/;

export function findWgslSymbols(text: string): ShaderSymbol[] {
  const symbols: ShaderSymbol[] = [];

  text.split('\n').forEach((line, index) => {
    const fn = WGSL_FN.exec(line);
    if (fn) {
      symbols.push({ name: fn[1], kind: 'function', line: index });
      return;
    }
    const struct = WGSL_STRUCT.exec(line);
    if (struct) {
      symbols.push({ name: struct[1], kind: 'struct', line: index });
    }
  });

  return symbols;
}

// ── GLSL ───────────────────────────────────────────────────────────

/** Qualifiers that may precede a declaration's type. */
const QUALIFIER =
  '(?:const|uniform|buffer|shared|in|out|inout|attribute|varying|centroid|flat|smooth|noperspective|invariant|precise|patch|sample|coherent|volatile|restrict|readonly|writeonly|highp|mediump|lowp|subroutine)';

/** An optional `layout(...)` prefix, which may carry nested parentheses in ids. */
const LAYOUT = '(?:layout\\s*\\([^)]*\\)\\s*)?';

const GLSL_STRUCT = new RegExp(`^\\s*struct\\s+([A-Za-z_][A-Za-z0-9_]*)`);

/** `layout(std140, binding = 0) uniform Camera {` — a named interface block. */
const GLSL_BLOCK = new RegExp(
  `^\\s*${LAYOUT}(?:${QUALIFIER}\\s+)*(?:uniform|buffer|in|out)\\s+([A-Za-z_][A-Za-z0-9_]*)\\s*\\{`,
);

/** `vec4 shade(vec3 n)` — a definition, not a prototype, so no trailing `;`. */
const GLSL_FUNCTION = new RegExp(
  `^\\s*(?:${QUALIFIER}\\s+)*([A-Za-z_][A-Za-z0-9_]*)\\s+([A-Za-z_][A-Za-z0-9_]*)\\s*\\(`,
);

/** `layout(location = 0) in vec3 position;` — a qualified global. */
const GLSL_GLOBAL = new RegExp(
  `^\\s*${LAYOUT}(?:${QUALIFIER}\\s+)+[A-Za-z_][A-Za-z0-9_]*\\s+([A-Za-z_][A-Za-z0-9_]*)\\s*(?:\\[[^\\]]*\\]\\s*)?[;=]`,
);

/** Words that start a statement and would otherwise look like a return type. */
const GLSL_STATEMENT_KEYWORDS = new Set([
  'if', 'else', 'for', 'while', 'do', 'switch', 'case', 'default',
  'return', 'break', 'continue', 'discard', 'struct', 'layout', 'precision',
]);

export function findGlslSymbols(text: string): ShaderSymbol[] {
  const symbols: ShaderSymbol[] = [];

  text.split('\n').forEach((line, index) => {
    const code = stripLineComment(line);

    const struct = GLSL_STRUCT.exec(code);
    if (struct) {
      symbols.push({ name: struct[1], kind: 'struct', line: index });
      return;
    }

    const block = GLSL_BLOCK.exec(code);
    if (block) {
      symbols.push({ name: block[1], kind: 'struct', line: index });
      return;
    }

    const global = GLSL_GLOBAL.exec(code);
    if (global) {
      symbols.push({ name: global[1], kind: 'variable', line: index });
      return;
    }

    // A prototype ends in `;` and declares nothing worth listing twice.
    if (code.trimEnd().endsWith(';')) return;

    const fn = GLSL_FUNCTION.exec(code);
    if (fn && !GLSL_STATEMENT_KEYWORDS.has(fn[1])) {
      symbols.push({ name: fn[2], kind: 'function', line: index });
    }
  });

  return symbols;
}

/** Drop a trailing `//` comment so its words cannot look like declarations. */
function stripLineComment(line: string): string {
  const index = line.indexOf('//');
  return index === -1 ? line : line.slice(0, index);
}
