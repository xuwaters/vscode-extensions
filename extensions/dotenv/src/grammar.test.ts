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
      if (scopeName !== 'source.dotenv') return null;
      const file = path.join(EXT_ROOT, 'syntaxes', 'dotenv.tmLanguage.json');
      return textmate.parseRawGrammar(fs.readFileSync(file, 'utf8'), file);
    },
  });
  const loaded = await registry.loadGrammar('source.dotenv');
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

describe('dotenv grammar', () => {
  it('keeps an apostrophe inside an unquoted value', () => {
    const src = "SHOP=Okoro's Provisions\nSKILL=Building\nERR=This week's paid requests are done.\n";
    expect(scopeOf(src, "'")).toBe('string.unquoted.dotenv');
    expect(scopeOf(src, 'Provisions')).toBe('string.unquoted.dotenv');
    expect(scopeOf(src, 'SKILL')).toBe('variable.other.dotenv');
    expect(scopeOf(src, 'Building')).toBe('string.unquoted.dotenv');
    expect(scopeOf(src, "'s paid")).toBe('string.unquoted.dotenv');
  });

  it('keeps other quote characters inside an unquoted value', () => {
    const src = 'A=say "hi" and `ok`\nB=1\n';
    expect(scopeOf(src, '"hi"')).toBe('string.unquoted.dotenv');
    expect(scopeOf(src, '`ok`')).toBe('string.unquoted.dotenv');
    expect(scopeOf(src, 'B')).toBe('variable.other.dotenv');
  });

  it('scopes quoted values that open the value', () => {
    const src = "S='it is' # note\nD = \"x ${S}\"\nT=`t`\n";
    expect(scopeOf(src, "'")).toBe('punctuation.definition.string.begin.dotenv');
    expect(scopeOf(src, 'it is')).toBe('string.quoted.single.dotenv');
    expect(scopeOf(src, '# note')).toBe('punctuation.definition.comment.dotenv');
    expect(scopeOf(src, '"')).toBe('punctuation.definition.string.begin.dotenv');
    expect(scopeOf(src, 'S}')).toBe('variable.other.name.dotenv');
    expect(scopeOf(src, 't`')).toBe('string.interpolated.dotenv');
  });

  it('scopes references and comments in an unquoted value', () => {
    const src = 'A=pre-$HOME-${USER}#x # tail\nB=cost$5\n';
    expect(scopeOf(src, 'HOME')).toBe('variable.other.name.dotenv');
    expect(scopeOf(src, 'USER')).toBe('variable.other.name.dotenv');
    expect(scopeOf(src, '#x')).toBe('string.unquoted.dotenv');
    expect(scopeOf(src, '# tail')).toBe('punctuation.definition.comment.dotenv');
    expect(scopeOf(src, '$5')).toBe('string.unquoted.dotenv');
  });
});
