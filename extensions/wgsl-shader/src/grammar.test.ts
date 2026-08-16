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

let registry: textmate.Registry;

beforeAll(async () => {
  const wasmPath = path.join(
    EXT_ROOT,
    'node_modules',
    'vscode-oniguruma',
    'release',
    'onig.wasm',
  );
  await oniguruma.loadWASM(fs.readFileSync(wasmPath).buffer as ArrayBuffer);

  registry = new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: (sources) => new oniguruma.OnigScanner(sources),
      createOnigString: (str) => new oniguruma.OnigString(str),
    }),
    loadGrammar: async (scopeName) => {
      if (scopeName === 'source.rust') {
        return textmate.parseRawGrammar(RUST_STUB, 'rust-stub.json');
      }
      const file = GRAMMARS[scopeName];
      if (!file) return null;
      const raw = fs.readFileSync(path.join(EXT_ROOT, file), 'utf8');
      return textmate.parseRawGrammar(raw, file);
    },
    getInjections: (scopeName) =>
      scopeName === 'source.rust' ? ['inline.rust.wgsl'] : undefined,
  });
});

/** Tokenize `source` as Rust and return, per line, the scopes on each token. */
async function tokenizeRust(source: string): Promise<textmate.IToken[][]> {
  const grammar = await registry.loadGrammar('source.rust');
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

async function embeddedScopes(source: string, needle: string): Promise<string[]> {
  const lines = source.split('\n');
  const tokenized = await tokenizeRust(source);
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
