import { describe, expect, it, beforeAll } from 'vitest';
import * as path from 'path';
import * as fs from 'fs';
import * as oniguruma from 'vscode-oniguruma';
import * as textmate from 'vscode-textmate';

const EXT_ROOT = path.join(__dirname, '..');
const EMBEDDED = 'meta.embedded.block.wgsl';

const GRAMMARS: Record<string, string> = {
  'source.wgsl': 'syntaxes/wgsl.tmLanguage.json',
  'inline.wgsl': 'syntaxes/wgsl-injection.tmLanguage.json',
  'inline.rust.wgsl': 'syntaxes/wgsl-rust-injection.tmLanguage.json',
};

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

/** Registry whose `source.rust` is `rustGrammar`, with our injection wired in. */
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
    getInjections: (scopeName) =>
      scopeName === 'source.rust' ? ['inline.rust.wgsl'] : undefined,
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

/** Tokenize `source` as Rust and return, per line, the scopes on each token. */
async function tokenizeRust(
  source: string,
  reg: textmate.Registry = registry,
): Promise<textmate.IToken[][]> {
  const grammar = await reg.loadGrammar('source.rust');
  expect(grammar).not.toBeNull();
  let ruleStack = textmate.INITIAL;
  const lines: textmate.IToken[][] = [];
  for (const line of source.split('\n')) {
    const result = grammar!.tokenizeLine(line, ruleStack);
    ruleStack = result.ruleStack;
    lines.push(result.tokens);
  }
  return lines;
}

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

describe('WGSL injection into Rust', () => {
  it('marks a hashed raw string tagged with /* wgsl */ as embedded WGSL', async () => {
    const scopes = await embeddedScopes(
      ['const S: &str = /* wgsl */ r#"', '  fn main() {}', '"#;'].join('\n'),
      'fn main',
    );
    expect(scopes).toContain(EMBEDDED);
  });

  it('handles multiple hashes and closes on the matching delimiter', async () => {
    const source = [
      'const S: &str = /* wgsl */ r##"',
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
    const source = ['const S: &str = /* wgsl */ r"', '  fn vs_main() {}', '";', 'let after = 1;'].join('\n');
    expect(await embeddedScopes(source, 'fn vs_main')).toContain(EMBEDDED);
    expect(await embeddedScopes(source, 'let after')).not.toContain(EMBEDDED);
  });

  it('supports plain string literals and keeps Rust escape scopes', async () => {
    const source = ['const S: &str = /* wgsl */ "', '  fn cs_main() {}\\n', '";'].join('\n');
    expect(await embeddedScopes(source, 'fn cs_main')).toContain(EMBEDDED);
    expect(await embeddedScopes(source, '\\n')).toContain('constant.character.escape.rust');
  });

  it('accepts comment spelling variations', async () => {
    for (const tag of ['/*wgsl*/', '/*  wgsl  */', '/* wgsl */']) {
      const scopes = await embeddedScopes(
        [`const S: &str = ${tag} r#"`, '  fn f() {}', '"#;'].join('\n'),
        'fn f',
      );
      expect(scopes, tag).toContain(EMBEDDED);
    }
  });

  it('leaves untagged strings alone', async () => {
    const scopes = await embeddedScopes(
      ['const S: &str = r#"', '  fn main() {}', '"#;'].join('\n'),
      'fn main',
    );
    expect(scopes).not.toContain(EMBEDDED);
  });

  it('does not treat a /* wgsl */ comment far from a string as a tag', async () => {
    const scopes = await embeddedScopes(
      ['/* wgsl */', 'const S: &str = r#"', '  fn main() {}', '"#;'].join('\n'),
      'fn main',
    );
    expect(scopes).not.toContain(EMBEDDED);
  });

  it('highlights the example file', async () => {
    const source = fs.readFileSync(path.join(EXT_ROOT, 'examples', 'test-embedded.rs'), 'utf8');
    expect(await embeddedScopes(source, '@compute')).toContain(EMBEDDED);
    expect(await embeddedScopes(source, 'fn main() {')).not.toContain(EMBEDDED);
  });
});

// The stub above cannot show how the injection competes with Rust's own comment
// and string rules, which is where an injection normally goes wrong.
describe.skipIf(!REAL_RUST_GRAMMAR)("against VS Code's real Rust grammar", () => {
  const example = () =>
    fs.readFileSync(path.join(EXT_ROOT, 'examples', 'test-embedded.rs'), 'utf8');

  /** Every line of examples/test-embedded.rs, flagged as embedded WGSL or not. */
  async function embeddedLines(): Promise<boolean[]> {
    const tokenized = await tokenizeRust(example(), realRegistry);
    return tokenized.map((tokens) => tokens.some((t) => t.scopes.includes(EMBEDDED)));
  }

  it('marks exactly the shader bodies in the example file', async () => {
    const flags = await embeddedLines();
    const lines = example().split('\n');
    const embedded = lines.filter((_, i) => flags[i]);
    const plain = lines.filter((_, i) => !flags[i]);

    // The three shader bodies, and nothing else.
    for (const needle of ['struct VertexOutput', 'fn fs_clear', '@compute', 'data[id.x]']) {
      expect(embedded.some((l) => l.includes(needle)), needle).toBe(true);
    }
    for (const needle of ['const SHADER', 'const CLEAR', 'const COMPUTE', 'fn main() {', 'println!']) {
      expect(plain.some((l) => l.includes(needle)), needle).toBe(true);
    }
  });

  it('keeps the tag comment and string delimiters scoped as Rust', async () => {
    const lines = example().split('\n');
    const tokenized = await tokenizeRust(example(), realRegistry);
    const at = (lineNeedle: string, needle: string) => {
      const i = lines.findIndex((l) => l.includes(lineNeedle));
      expect(i, lineNeedle).toBeGreaterThanOrEqual(0);
      return scopesAt(tokenized[i], lines[i], needle);
    };

    expect(at('const SHADER', '/* wgsl */')).toContain('comment.block.rust');
    expect(at('const SHADER', 'r#"')).toContain('punctuation.definition.string.raw.begin.rust');
    expect(at('"#;', '"#;')).toContain('punctuation.definition.string.raw.end.rust');
  });

  it('resolves WGSL scopes inside the shader bodies', async () => {
    const source = example();
    expect(await embeddedScopes(source, 'struct VertexOutput', realRegistry)).toContain(
      'storage.type.wgsl',
    );
    expect(await embeddedScopes(source, '@group(0)', realRegistry)).toContain(
      'keyword.operator.attribute.at.wgsl',
    );
  });
});
