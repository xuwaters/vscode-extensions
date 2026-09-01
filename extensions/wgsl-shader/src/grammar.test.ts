import { describe, expect, it, beforeAll } from 'vitest';
import * as path from 'path';
import * as fs from 'fs';
import * as oniguruma from 'vscode-oniguruma';
import * as textmate from 'vscode-textmate';

const EXT_ROOT = path.join(__dirname, '..');

const GRAMMARS: Record<string, string> = {
  'source.wgsl': 'syntaxes/wgsl.tmLanguage.json',
  'inline.wgsl': 'syntaxes/wgsl-injection.tmLanguage.json',
  'inline.rust.wgsl': 'syntaxes/wgsl-rust-injection.tmLanguage.json',
  'source.glsl': 'syntaxes/glsl.tmLanguage.json',
  'inline.glsl': 'syntaxes/glsl-injection.tmLanguage.json',
  'inline.rust.glsl': 'syntaxes/glsl-rust-injection.tmLanguage.json',
};

const RUST_INJECTIONS = ['inline.rust.wgsl', 'inline.rust.glsl'];

/** The two shader languages, and the fixtures the injection tests run against. */
const LANGUAGES = [
  {
    id: 'wgsl',
    embedded: 'meta.embedded.block.wgsl',
    example: 'test-embedded.rs',
    // Lines of the example file that are shader, and lines that are not.
    shaderLines: ['struct VertexOutput', 'fn fs_clear', '@compute', 'data[id.x]'],
    hostLines: ['const SHADER', 'const CLEAR', 'const COMPUTE', 'fn main() {', 'println!'],
  },
  {
    id: 'glsl',
    embedded: 'meta.embedded.block.glsl',
    example: 'test-embedded-glsl.rs',
    shaderLines: ['#version 450', 'out_color = vec4(0.0', 'local_size_x', 'values[i]'],
    hostLines: ['const VERTEX', 'const CLEAR', 'const COMPUTE', 'fn main() {', 'println!'],
  },
] as const;

// `source.rust` is not bundled here; a stub host grammar is enough to exercise
// the injection, which is the only thing this extension contributes to Rust.
const RUST_STUB = JSON.stringify({
  scopeName: 'source.rust',
  patterns: [{ match: '\\w+', name: 'meta.word.rust' }],
});

/** VS Code ships the Rust grammar; use the real one when this machine has it. */
const REAL_RUST_GRAMMAR = [
  '/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/rust/syntaxes/rust.tmLanguage.json',
  '/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/extensions/rust/syntaxes/rust.tmLanguage.json',
  '/Applications/Cursor.app/Contents/Resources/app/extensions/rust/syntaxes/rust.tmLanguage.json',
  '/usr/share/code/resources/app/extensions/rust/syntaxes/rust.tmLanguage.json',
].find((p) => fs.existsSync(p));

let registry: textmate.Registry;
let realRegistry: textmate.Registry | undefined;

/** Registry whose `source.rust` is `rustGrammar`, with our injections wired in. */
function makeRegistry(rustGrammar: string, rustGrammarPath: string): textmate.Registry {
  return new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: (sources) => new oniguruma.OnigScanner(sources),
      createOnigString: (str) => new oniguruma.OnigString(str),
    }),
    loadGrammar: async (scopeName) => {
      if (scopeName === 'source.rust') {
        return textmate.parseRawGrammar(rustGrammar, rustGrammarPath);
      }
      const file = GRAMMARS[scopeName];
      if (!file) return null;
      const raw = fs.readFileSync(path.join(EXT_ROOT, file), 'utf8');
      return textmate.parseRawGrammar(raw, file);
    },
    getInjections: (scopeName) => (scopeName === 'source.rust' ? RUST_INJECTIONS : undefined),
  });
}

beforeAll(async () => {
  const wasmPath = path.join(
    EXT_ROOT,
    'node_modules',
    'vscode-oniguruma',
    'release',
    'onig.wasm',
  );
  await oniguruma.loadWASM(fs.readFileSync(wasmPath).buffer as ArrayBuffer);

  registry = makeRegistry(RUST_STUB, 'rust-stub.json');
  if (REAL_RUST_GRAMMAR) {
    realRegistry = makeRegistry(fs.readFileSync(REAL_RUST_GRAMMAR, 'utf8'), REAL_RUST_GRAMMAR);
  }
});

/** Tokenize `source` under `scopeName` and return, per line, the tokens on it. */
async function tokenize(
  source: string,
  scopeName: string,
  reg: textmate.Registry = registry,
): Promise<textmate.IToken[][]> {
  const grammar = await reg.loadGrammar(scopeName);
  expect(grammar, scopeName).not.toBeNull();
  let ruleStack = textmate.INITIAL;
  const lines: textmate.IToken[][] = [];
  for (const line of source.split('\n')) {
    const result = grammar!.tokenizeLine(line, ruleStack);
    ruleStack = result.ruleStack;
    lines.push(result.tokens);
  }
  return lines;
}

const tokenizeRust = (source: string, reg?: textmate.Registry) =>
  tokenize(source, 'source.rust', reg);

/** Scopes of the token covering `needle` on the given line. */
function scopesAt(tokens: textmate.IToken[], line: string, needle: string): string[] {
  const index = line.indexOf(needle);
  expect(index, `"${needle}" not found in ${JSON.stringify(line)}`).toBeGreaterThanOrEqual(0);
  const token = tokens.find((t) => t.startIndex <= index && index < t.endIndex);
  expect(token, `no token at column ${index}`).toBeDefined();
  return token!.scopes;
}

async function embeddedScopes(
  source: string,
  needle: string,
  reg?: textmate.Registry,
): Promise<string[]> {
  const lines = source.split('\n');
  const tokenized = await tokenizeRust(source, reg);
  const lineIndex = lines.findIndex((l) => l.includes(needle));
  expect(lineIndex, `"${needle}" not found in source`).toBeGreaterThanOrEqual(0);
  return scopesAt(tokenized[lineIndex], lines[lineIndex], needle);
}

describe.each(LANGUAGES)('$id injection into Rust', ({ id, embedded, example }) => {
  const EMBEDDED = embedded;

  it('marks a hashed raw string tagged with the language as embedded', async () => {
    const scopes = await embeddedScopes(
      [`const S: &str = /* ${id} */ r#"`, '  vec4 x;', '"#;'].join('\n'),
      'vec4 x',
    );
    expect(scopes).toContain(EMBEDDED);
  });

  it('handles multiple hashes and closes on the matching delimiter', async () => {
    const source = [
      `const S: &str = /* ${id} */ r##"`,
      '  let a = 1.0;',
      '"#',
      '  let b = 2.0;',
      '"##;',
      'let outside = 3;',
    ].join('\n');
    expect(await embeddedScopes(source, 'let a')).toContain(EMBEDDED);
    // A shorter `"#` does not terminate a `r##"` string.
    expect(await embeddedScopes(source, 'let b')).toContain(EMBEDDED);
    expect(await embeddedScopes(source, 'let outside')).not.toContain(EMBEDDED);
  });

  it('supports raw strings without hashes', async () => {
    const source = [
      `const S: &str = /* ${id} */ r"`,
      '  vec2 uv;',
      '";',
      'let after = 1;',
    ].join('\n');
    expect(await embeddedScopes(source, 'vec2 uv')).toContain(EMBEDDED);
    expect(await embeddedScopes(source, 'let after')).not.toContain(EMBEDDED);
  });

  it('supports plain string literals and keeps Rust escape scopes', async () => {
    const source = [`const S: &str = /* ${id} */ "`, '  vec3 n;\\n', '";'].join('\n');
    expect(await embeddedScopes(source, 'vec3 n')).toContain(EMBEDDED);
    expect(await embeddedScopes(source, '\\n')).toContain('constant.character.escape.rust');
  });

  it('accepts comment spelling variations', async () => {
    for (const tag of [`/*${id}*/`, `/*  ${id}  */`, `/* ${id} */`]) {
      const scopes = await embeddedScopes(
        [`const S: &str = ${tag} r#"`, '  float f;', '"#;'].join('\n'),
        'float f',
      );
      expect(scopes, tag).toContain(EMBEDDED);
    }
  });

  it('leaves untagged strings alone', async () => {
    const scopes = await embeddedScopes(
      ['const S: &str = r#"', '  vec4 x;', '"#;'].join('\n'),
      'vec4 x',
    );
    expect(scopes).not.toContain(EMBEDDED);
  });

  it('does not treat a tag comment far from a string as a tag', async () => {
    const scopes = await embeddedScopes(
      [`/* ${id} */`, 'const S: &str = r#"', '  vec4 x;', '"#;'].join('\n'),
      'vec4 x',
    );
    expect(scopes).not.toContain(EMBEDDED);
  });

  it('does not pick up the other language\'s tag', async () => {
    const other = id === 'wgsl' ? 'glsl' : 'wgsl';
    const scopes = await embeddedScopes(
      [`const S: &str = /* ${other} */ r#"`, '  vec4 x;', '"#;'].join('\n'),
      'vec4 x',
    );
    expect(scopes).not.toContain(EMBEDDED);
  });

  it('highlights the example file', async () => {
    const source = fs.readFileSync(path.join(EXT_ROOT, 'examples', example), 'utf8');
    expect(await embeddedScopes(source, 'fn main() {')).not.toContain(EMBEDDED);
  });
});

// The stub above cannot show how the injection competes with Rust's own comment
// and string rules, which is where an injection normally goes wrong.
describe.skipIf(!REAL_RUST_GRAMMAR).each(LANGUAGES)(
  "$id against VS Code's real Rust grammar",
  ({ embedded, example, shaderLines, hostLines }) => {
    const EMBEDDED = embedded;
    const source = () => fs.readFileSync(path.join(EXT_ROOT, 'examples', example), 'utf8');

    it('marks exactly the shader bodies in the example file', async () => {
      const tokenized = await tokenizeRust(source(), realRegistry);
      const flags = tokenized.map((tokens) =>
        tokens.some((t) => t.scopes.includes(EMBEDDED)),
      );
      const lines = source().split('\n');
      const inShader = lines.filter((_, i) => flags[i]);
      const inHost = lines.filter((_, i) => !flags[i]);

      for (const needle of shaderLines) {
        expect(inShader.some((l) => l.includes(needle)), needle).toBe(true);
      }
      for (const needle of hostLines) {
        expect(inHost.some((l) => l.includes(needle)), needle).toBe(true);
      }
    });

    it('keeps the tag comment and string delimiters scoped as Rust', async () => {
      const lines = source().split('\n');
      const tokenized = await tokenizeRust(source(), realRegistry);
      const at = (lineNeedle: string, needle: string) => {
        const i = lines.findIndex((l) => l.includes(lineNeedle));
        expect(i, lineNeedle).toBeGreaterThanOrEqual(0);
        return scopesAt(tokenized[i], lines[i], needle);
      };

      const tagLine = lines.find((l) => l.includes('r#"'))!;
      expect(at(tagLine, '/*')).toContain('comment.block.rust');
      expect(at(tagLine, 'r#"')).toContain('punctuation.definition.string.raw.begin.rust');
      expect(at('"#;', '"#;')).toContain('punctuation.definition.string.raw.end.rust');
    });
  },
);

describe("WGSL scopes resolve inside a Rust string", () => {
  it.skipIf(!REAL_RUST_GRAMMAR)('colours declarations and attributes', async () => {
    const source = fs.readFileSync(path.join(EXT_ROOT, 'examples', 'test-embedded.rs'), 'utf8');
    expect(await embeddedScopes(source, 'struct VertexOutput', realRegistry)).toContain(
      'storage.type.wgsl',
    );
    expect(await embeddedScopes(source, '@group(0)', realRegistry)).toContain(
      'keyword.operator.attribute.at.wgsl',
    );
  });
});

describe("GLSL scopes resolve inside a Rust string", () => {
  it.skipIf(!REAL_RUST_GRAMMAR)('colours directives and qualifiers', async () => {
    const source = fs.readFileSync(
      path.join(EXT_ROOT, 'examples', 'test-embedded-glsl.rs'),
      'utf8',
    );
    expect(await embeddedScopes(source, '#version 450', realRegistry)).toContain(
      'keyword.control.directive.glsl',
    );
    expect(await embeddedScopes(source, 'layout(location = 0) in vec3', realRegistry)).toContain(
      'storage.modifier.glsl',
    );
  });
});

describe('GLSL grammar', () => {
  /** Scopes of the token covering `needle`, tokenizing `source` as GLSL. */
  async function glslScopes(source: string, needle: string): Promise<string[]> {
    const lines = source.split('\n');
    const tokenized = await tokenize(source, 'source.glsl');
    const i = lines.findIndex((l) => l.includes(needle));
    expect(i, `"${needle}" not found in source`).toBeGreaterThanOrEqual(0);
    return scopesAt(tokenized[i], lines[i], needle);
  }

  it('scopes preprocessor directives', async () => {
    expect(await glslScopes('#version 450 core\n', '#version')).toContain(
      'keyword.control.directive.glsl',
    );
    expect(await glslScopes('#version 450 core\n', 'core')).toContain('constant.language.glsl');
    expect(await glslScopes('#define MAX 4\n', '#define')).toContain(
      'keyword.control.directive.glsl',
    );
    // The directive ends at the newline; the next line is ordinary code.
    expect(await glslScopes('#define MAX 4\nvoid main() {}\n', 'void')).toContain(
      'storage.type.glsl',
    );
  });

  it('scopes types, qualifiers and layout ids', async () => {
    const source = 'layout(location = 0) in vec3 in_position;\n';
    expect(await glslScopes(source, 'layout')).toContain('storage.modifier.glsl');
    expect(await glslScopes(source, 'location')).toContain('entity.name.attribute.glsl');
    expect(await glslScopes('uniform sampler2D tex;\n', 'sampler2D')).toContain(
      'storage.type.glsl',
    );
    expect(await glslScopes('uniform texture2D tex;\n', 'texture2D')).toContain(
      'storage.type.glsl',
    );
    expect(await glslScopes('  mat4 m;\n', 'mat4')).toContain('storage.type.glsl');
    expect(await glslScopes('  dvec3 v;\n', 'dvec3')).toContain('storage.type.glsl');
  });

  it('scopes gl_ built-ins apart from user variables', async () => {
    const source = '  gl_Position = position;\n';
    expect(await glslScopes(source, 'gl_Position')).toContain('support.variable.builtin.glsl');
    expect(await glslScopes(source, 'position;')).toContain('variable.other.glsl');
  });

  it('scopes built-in calls apart from user calls', async () => {
    expect(await glslScopes('  float d = dot(a, b);\n', 'dot(')).toContain(
      'support.function.builtin.glsl',
    );
    expect(await glslScopes('  float d = shade(a, b);\n', 'shade(')).toContain(
      'entity.name.function.glsl',
    );
  });

  it('scopes function definitions', async () => {
    const scopes = await glslScopes('vec4 shade(vec3 n) {\n', 'shade');
    expect(scopes).toContain('entity.name.function.glsl');
  });

  it('scopes comments and does not read code inside them', async () => {
    expect(await glslScopes('// gl_Position\n', 'gl_Position')).toContain(
      'comment.line.double-slash.glsl',
    );
    expect(await glslScopes('/* vec4 */\n', 'vec4')).toContain('comment.block.glsl');
  });

  it('scopes numeric literals', async () => {
    expect(await glslScopes('  float a = 1.5;\n', '1.5')).toContain(
      'constant.numeric.float.glsl',
    );
    expect(await glslScopes('  float a = 2.0f;\n', '2.0f')).toContain(
      'constant.numeric.float.glsl',
    );
    expect(await glslScopes('  uint a = 0xffu;\n', '0xff')).toContain(
      'constant.numeric.hex.glsl',
    );
    expect(await glslScopes('  int a = 42;\n', '42')).toContain('constant.numeric.decimal.glsl');
  });

  it('tokenizes the example shaders without falling into a comment or string', async () => {
    const files = ['test.vert', 'test.frag', 'test.comp', 'test-es300.frag', 'test-opengl.frag'];
    for (const file of files) {
      const source = fs.readFileSync(path.join(EXT_ROOT, 'examples', file), 'utf8');
      const tokenized = await tokenize(source, 'source.glsl');
      const lines = source.split('\n');
      const lastLine = lines.length - 2; // the file ends with a newline
      const stuck = tokenized[lastLine].some((t) =>
        t.scopes.some((s) => s.startsWith('comment') || s.startsWith('string')),
      );
      expect(stuck, `${file} ends inside a comment or string`).toBe(false);
    }
  });
});
