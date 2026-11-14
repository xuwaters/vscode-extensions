import * as fs from 'fs';
import * as path from 'path';
import * as oniguruma from 'vscode-oniguruma';
import * as textmate from 'vscode-textmate';
import { beforeAll, describe, expect, it } from 'vitest';

const EXT_ROOT = path.join(__dirname, '..');

let grammar: textmate.IGrammar;

beforeAll(async () => {
  const wasm = path.join(EXT_ROOT, 'node_modules', 'vscode-oniguruma', 'release', 'onig.wasm');
  await oniguruma.loadWASM(fs.readFileSync(wasm).buffer as ArrayBuffer);
  const registry = new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: (sources) => new oniguruma.OnigScanner(sources),
      createOnigString: (str) => new oniguruma.OnigString(str),
    }),
    loadGrammar: async (scopeName) => {
      if (scopeName !== 'source.ron') return null;
      const file = path.join(EXT_ROOT, 'syntaxes', 'ron.tmLanguage.json');
      return textmate.parseRawGrammar(fs.readFileSync(file, 'utf8'), file);
    },
  });
  const loaded = await registry.loadGrammar('source.ron');
  expect(loaded).not.toBeNull();
  grammar = loaded!;
});

/** The innermost scope of the token covering `needle` (searched after `from`). */
function scopeOf(source: string, needle: string, from = 0): string {
  const offset = source.indexOf(needle, from);
  expect(offset, `"${needle}" not found`).toBeGreaterThanOrEqual(0);
  let ruleStack = textmate.INITIAL;
  let lineStart = 0;
  for (const line of source.split('\n')) {
    const { tokens, ruleStack: next } = grammar.tokenizeLine(line, ruleStack);
    ruleStack = next;
    const col = offset - lineStart;
    if (col >= 0 && col < line.length) {
      const token = tokens.find((t) => t.startIndex <= col && col < t.endIndex)!;
      return token.scopes[token.scopes.length - 1];
    }
    lineStart += line.length + 1;
  }
  throw new Error(`no token at ${offset}`);
}

describe('RON grammar', () => {
  it('scopes structure', () => {
    const src = 'Config(window: (w: 800), mode: Fullscreen, opt: Some(1), none: None)';
    expect(scopeOf(src, 'Config')).toBe('entity.name.type.struct.ron');
    expect(scopeOf(src, 'window')).toBe('variable.other.member.ron');
    expect(scopeOf(src, ':')).toBe('punctuation.separator.key-value.ron');
    expect(scopeOf(src, 'Fullscreen')).toBe('entity.name.type.ron');
    expect(scopeOf(src, 'Some')).toBe('constant.language.option.ron');
    expect(scopeOf(src, 'None')).toBe('constant.language.option.ron');
    expect(scopeOf(src, '(')).toBe('punctuation.brackets.round.ron');
  });

  it('scopes numbers', () => {
    const cases: Array<[string, string]> = [
      ['42', 'constant.numeric.integer.decimal.ron'],
      ['-7i64', 'constant.numeric.integer.decimal.ron'],
      ['0xFF_u8', 'constant.numeric.integer.hexadecimal.ron'],
      ['0o17', 'constant.numeric.integer.octal.ron'],
      ['0b1010', 'constant.numeric.integer.binary.ron'],
      ['1.5e-3', 'constant.numeric.float.ron'],
      ['.5', 'constant.numeric.float.ron'],
      ['2f32', 'constant.numeric.float.ron'],
      ['-inf', 'constant.numeric.float.ron'],
      ['NaN', 'constant.numeric.float.ron'],
    ];
    for (const [literal, scope] of cases) {
      const src = `[${literal}]`;
      expect(scopeOf(src, literal), literal).toBe(scope);
    }
  });

  it('scopes strings, escapes and chars', () => {
    const src = '("a\\n", r#"raw "q""#, b"\\xff", \'x\', \'\\u{1F980}\', "bad\\q")';
    expect(scopeOf(src, '"a')).toBe('punctuation.definition.string.begin.ron');
    expect(scopeOf(src, 'a\\n')).toBe('string.quoted.double.ron');
    expect(scopeOf(src, '\\n')).toBe('constant.character.escape.ron');
    expect(scopeOf(src, 'raw')).toBe('string.quoted.double.raw.ron');
    expect(scopeOf(src, 'q"')).toBe('string.quoted.double.raw.ron');
    expect(scopeOf(src, '\\xff')).toBe('constant.character.escape.ron');
    expect(scopeOf(src, "'x'")).toBe('string.quoted.single.char.ron');
    expect(scopeOf(src, '\\q')).toBe('invalid.illegal.escape.ron');
  });

  it('keeps multi-line strings and nested comments open', () => {
    const src = '(a: "one\ntwo", /* x /* y */ still */ b: 1)';
    expect(scopeOf(src, 'two')).toBe('string.quoted.double.ron');
    expect(scopeOf(src, 'still')).toBe('comment.block.ron');
    expect(scopeOf(src, 'b:')).toBe('variable.other.member.ron');
  });

  it('scopes comments and attributes', () => {
    const src = '#![enable(implicit_some, nope)] // tail\n5';
    expect(scopeOf(src, '#')).toBe('punctuation.definition.attribute.ron');
    expect(scopeOf(src, 'enable')).toBe('keyword.other.attribute.ron');
    expect(scopeOf(src, 'implicit_some')).toBe('support.constant.extension.ron');
    expect(scopeOf(src, 'nope')).toBe('invalid.illegal.extension.ron');
    expect(scopeOf(src, 'tail')).toBe('comment.line.double-slash.ron');
  });

  it('scopes raw identifiers as fields', () => {
    expect(scopeOf('(r#type: 1)', 'r#type')).toBe('variable.other.member.ron');
  });
});
