import * as fs from 'node:fs';
import * as path from 'node:path';
import { beforeAll, describe, expect, it } from 'vitest';
import * as oniguruma from 'vscode-oniguruma';
import * as textmate from 'vscode-textmate';

const EXT_ROOT = path.resolve(__dirname, '..');

// Cap'n Proto is the one host language whose grammar this extension bundles
// itself — nothing external has to resolve for these scopes to appear.
const GRAMMARS: Record<string, string> = {
  'source.askama': 'syntaxes/askama.tmLanguage.json',
  'source.capnp.askama': 'syntaxes/askama-capnp.tmLanguage.json',
  'askama.injection': 'syntaxes/askama-injection.tmLanguage.json',
};

/** As VS Code loads it: the language grammar with `askama.injection` wired in. */
let registry: textmate.Registry;
/** The language grammar on its own, for rules the injection outranks. */
let bareRegistry: textmate.Registry;

function makeRegistry(withInjection: boolean): textmate.Registry {
  return new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: sources => new oniguruma.OnigScanner(sources),
      createOnigString: str => new oniguruma.OnigString(str),
    }),
    loadGrammar: async scopeName => {
      const file = GRAMMARS[scopeName];
      if (!file) return null;
      return textmate.parseRawGrammar(fs.readFileSync(path.join(EXT_ROOT, file), 'utf8'), file);
    },
    getInjections: scopeName =>
      withInjection && scopeName === 'source.capnp.askama' ? ['askama.injection'] : undefined,
  });
}

beforeAll(async () => {
  const wasm = path.join(EXT_ROOT, 'node_modules', 'vscode-oniguruma', 'release', 'onig.wasm');
  await oniguruma.loadWASM(fs.readFileSync(wasm).buffer as ArrayBuffer);

  registry = makeRegistry(true);
  bareRegistry = makeRegistry(false);
});

async function tokenize(
  source: string,
  reg: textmate.Registry = registry,
): Promise<textmate.IToken[][]> {
  const grammar = await reg.loadGrammar('source.capnp.askama');
  expect(grammar).not.toBeNull();
  let ruleStack = textmate.INITIAL;
  return source.split('\n').map(line => {
    const result = grammar!.tokenizeLine(line, ruleStack);
    ruleStack = result.ruleStack;
    return result.tokens;
  });
}

/** Scopes of the token covering `needle`, which must appear on exactly one line. */
async function scopesOf(
  source: string,
  needle: string,
  reg?: textmate.Registry,
): Promise<string[]> {
  const lines = source.split('\n');
  const lineIndex = lines.findIndex(l => l.includes(needle));
  expect(lineIndex, `"${needle}" not found in source`).toBeGreaterThanOrEqual(0);
  const column = lines[lineIndex].indexOf(needle);
  const tokens = (await tokenize(source, reg))[lineIndex];
  const token = tokens.find(t => t.startIndex <= column && column < t.endIndex);
  expect(token, `no token at ${lineIndex}:${column}`).toBeDefined();
  return token!.scopes;
}

describe('askama-capnp grammar', () => {
  const schema = [
    '@0xdbb9ad1f14bf0b36;  # file id',
    'using Cxx = import "/capnp/c++.capnp";',
    '$Cxx.namespace("addressbook");',
    '',
    'struct Person {',
    '  id @0 :UInt32;',
    '  phones @1 :List(PhoneNumber);',
    '  employment :union {',
    '    unemployed @2 :Void;',
    '  }',
    '}',
    '',
    'interface Node {',
    '  read @0 (path :Text) -> (data :Data);',
    '}',
    '',
    'const pi :Float64 = 3.14159;',
    'enum Kind { mobile @0; }',
  ].join('\n');

  it('scopes declarations and their names', async () => {
    expect(await scopesOf(schema, 'struct Person')).toContain('storage.type.capnp');
    expect(await scopesOf(schema, 'Person {')).toContain('entity.name.type.capnp');
    expect(await scopesOf(schema, 'interface Node')).toContain('storage.type.capnp');
    expect(await scopesOf(schema, 'enum Kind')).toContain('storage.type.capnp');
  });

  it('scopes fields, ordinals and built-in types', async () => {
    expect(await scopesOf(schema, 'id @0')).toContain('variable.other.member.capnp');
    expect(await scopesOf(schema, '@0 :UInt32')).toContain('punctuation.definition.ordinal.capnp');
    expect(await scopesOf(schema, 'UInt32')).toContain('support.type.builtin.capnp');
    expect(await scopesOf(schema, 'List(')).toContain('support.type.builtin.capnp');
    expect(await scopesOf(schema, 'PhoneNumber')).toContain('entity.name.type.capnp');
    expect(await scopesOf(schema, 'mobile @0')).toContain('variable.other.member.capnp');
  });

  it('scopes unions, methods, annotations, strings and numbers', async () => {
    expect(await scopesOf(schema, 'union {')).toContain('storage.type.capnp');
    expect(await scopesOf(schema, 'read @0 (')).toContain('entity.name.function.capnp');
    expect(await scopesOf(schema, '$Cxx')).toContain('punctuation.definition.annotation.capnp');
    expect(await scopesOf(schema, 'Cxx.namespace')).toContain('entity.other.attribute-name.capnp');
    expect(await scopesOf(schema, '"/capnp/c++.capnp"')).toContain('string.quoted.double.capnp');
    expect(await scopesOf(schema, '3.14159')).toContain('constant.numeric.capnp');
    expect(await scopesOf(schema, 'import ')).toContain('keyword.control.import.capnp');
  });

  it('scopes # comments as Cap\'n Proto comments', async () => {
    expect(await scopesOf(schema, '# file id')).toContain('comment.line.number-sign.capnp');
  });

  // `{#` opens an Askama comment; the bare `#` rule must not win the race.
  it('prefers an Askama comment over a Cap\'n Proto comment at `{#`', async () => {
    const source = '{# a template comment #}\nstruct Foo {}';
    const scopes = await scopesOf(source, '{#');
    expect(scopes).toContain('comment.block.askama');
    expect(scopes).not.toContain('comment.line.number-sign.capnp');
    expect(await scopesOf(source, 'Foo')).toContain('entity.name.type.capnp');
  });

  it('scopes Askama statements and expressions between the schema', async () => {
    const source = [
      '{% for field in fields %}',
      '  {{ field.name }} @{{ loop.index0 }} :Text;',
      '{% endfor %}',
    ].join('\n');
    expect(await scopesOf(source, 'for')).toContain('keyword.control.askama');
    expect(await scopesOf(source, '{{ field')).toContain('meta.expression.askama');
    expect(await scopesOf(source, 'endfor')).toContain('keyword.control.askama');
  });

  it('injects Askama expressions inside Cap\'n Proto strings', async () => {
    const source = '$Cxx.namespace("{{ namespace }}");';
    const scopes = await scopesOf(source, '{{ namespace }}');
    expect(scopes).toContain('meta.expression.askama');
    expect(scopes).toContain('string.quoted.double.capnp');
  });

  const rawBlock = [
    '{% raw %}',
    'const literal :Text = "{{ stays_literal }}";',
    '{% endraw %}',
    'const after :UInt32 = {{ interpolated }};',
  ].join('\n');

  it('leaves template syntax alone inside a raw block, and keeps the schema lit', async () => {
    const inside = await scopesOf(rawBlock, '{{ stays_literal }}', bareRegistry);
    expect(inside).toContain('meta.raw.block.askama');
    expect(inside).not.toContain('meta.expression.askama');
    // Host highlighting still applies to the raw body.
    expect(await scopesOf(rawBlock, ':Text = ', bareRegistry)).toContain('punctuation.separator.capnp');
    expect(await scopesOf(rawBlock, '{{ interpolated }}', bareRegistry)).toContain(
      'meta.expression.askama',
    );
  });

  // `askama.injection` is registered with an `L:` selector, which outranks the
  // language grammar on a tie — so the injected statement rule, not the raw-block
  // rule, claims `{% raw %}`. That is how every language here has always behaved;
  // the decorations in extension.ts are what keep raw bodies uncoloured. Pinned so
  // a deliberate fix to the injection priority shows up as a failure here.
  it('has its raw block outranked by the injection, as the other languages do', async () => {
    expect(await scopesOf(rawBlock, '{% raw %}')).toContain('meta.statement.askama');
    expect(await scopesOf(rawBlock, '{{ stays_literal }}')).not.toContain('meta.raw.block.askama');
  });

  it('highlights the example file', async () => {
    const source = fs.readFileSync(path.join(EXT_ROOT, 'examples', 'example.capnp.askama'), 'utf8');
    expect(await scopesOf(source, 'struct Metadata')).toContain('storage.type.capnp');
    expect(await scopesOf(source, 'key @0')).toContain('variable.other.member.capnp');
    expect(await scopesOf(source, 'tags @2 :List(Text)')).toContain('variable.other.member.capnp');
    expect(await scopesOf(source, '{% match status %}')).toContain('meta.statement.askama');
    expect(await scopesOf(source, '{{ file_id }}')).toContain('meta.expression.askama');
  });
});
