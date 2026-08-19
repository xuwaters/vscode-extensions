import * as fs from 'node:fs';
import * as path from 'node:path';
import { beforeAll, describe, expect, it } from 'vitest';
import * as oniguruma from 'vscode-oniguruma';
import * as textmate from 'vscode-textmate';

/**
 * The grammar, tokenized for real.
 *
 * `src/embeddedLanguages.test.ts` checks the table and what is generated from
 * it as data. This runs it: the same Oniguruma the editor runs, over the same
 * grammar files, because the two constructs that make embedded raw blocks work
 * — `(?i:…)` and the back-reference that closes a fence with the run of
 * backticks that opened it — are Oniguruma-only and a JavaScript `RegExp`
 * cannot stand in for them.
 *
 * Languages other than typst are stubbed by default, so the suite passes on a
 * machine with no VS Code. Where a VS Code install is found, the second block
 * uses its real grammars — and checks the table's `provider` field against what
 * that install actually ships, which is the only way a wrong scope name
 * (`text.restructuredtext` for what is really `source.rst`) ever surfaces.
 */

const ROOT = path.join(__dirname, '..');
const EXTENSION_ID = 'wx-vsce-typst-ultra';

const OURS: Record<string, string> = {
  'source.typst': 'syntaxes/typst.tmLanguage.json',
  'source.typst.embedded': 'syntaxes/typst-embedded.tmLanguage.json',
};

interface EmbeddedLanguage {
  id: string;
  scope: string;
  tags: string[];
  provider?: 'extension' | 'self';
}

const LANGUAGES: EmbeddedLanguage[] = JSON.parse(
  fs.readFileSync(path.join(ROOT, 'scripts', 'embedded', 'languages.json'), 'utf8'),
).languages;

/** The `extensions` directory of a VS Code install, if this machine has one. */
const APP_EXTENSIONS = [
  '/Applications/Visual Studio Code.app/Contents/Resources/app/extensions',
  '/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/extensions',
  '/Applications/Cursor.app/Contents/Resources/app/extensions',
  '/usr/share/code/resources/app/extensions',
].find(candidate => fs.existsSync(candidate));

interface Builtins {
  /** TextMate scope → grammar file. */
  grammars: Map<string, string>;
  /** Every language id VS Code registers out of the box. */
  languageIds: Set<string>;
}

/** What the VS Code at `directory` contributes, read from its own manifests. */
function readBuiltins(directory: string): Builtins {
  const grammars = new Map<string, string>();
  const languageIds = new Set<string>();

  for (const name of fs.readdirSync(directory)) {
    const manifest = path.join(directory, name, 'package.json');
    if (!fs.existsSync(manifest)) continue;

    let contributes;
    try {
      contributes = JSON.parse(fs.readFileSync(manifest, 'utf8')).contributes;
    } catch {
      continue;
    }
    for (const grammar of contributes?.grammars ?? []) {
      grammars.set(grammar.scopeName, path.join(directory, name, grammar.path));
    }
    for (const language of contributes?.languages ?? []) {
      languageIds.add(language.id);
    }
  }

  return { grammars, languageIds };
}

const BUILTINS = APP_EXTENSIONS ? readBuiltins(APP_EXTENSIONS) : undefined;

/**
 * A registry whose typst grammars are ours and whose everything-else comes
 * from `resolve`.
 */
function makeRegistry(resolve: (scope: string) => string | undefined): textmate.Registry {
  return new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: sources => new oniguruma.OnigScanner(sources),
      createOnigString: source => new oniguruma.OnigString(source),
    }),
    loadGrammar: async scopeName => {
      const own = OURS[scopeName];
      if (own) {
        return textmate.parseRawGrammar(fs.readFileSync(path.join(ROOT, own), 'utf8'), own);
      }

      const file = resolve(scopeName);
      if (file) {
        return textmate.parseRawGrammar(fs.readFileSync(file, 'utf8'), file);
      }

      // A stub is enough to prove the block was handed over: it scopes every
      // word, so a token inside the fence carries the language's scope name.
      return textmate.parseRawGrammar(
        JSON.stringify({ scopeName, patterns: [{ match: '\\w+', name: `meta.word.${scopeName}` }] }),
        `${scopeName}-stub.json`,
      );
    },
  });
}

let stubbed: textmate.Registry;
let real: textmate.Registry | undefined;

beforeAll(async () => {
  const wasm = path.join(ROOT, 'node_modules', 'vscode-oniguruma', 'release', 'onig.wasm');
  await oniguruma.loadWASM(fs.readFileSync(wasm).buffer as ArrayBuffer);

  stubbed = makeRegistry(() => undefined);
  if (BUILTINS) real = makeRegistry(scope => BUILTINS.grammars.get(scope));
});

/** Every token of `source`, tokenized as typst, one array per line. */
async function tokenize(
  source: string,
  registry: textmate.Registry = stubbed,
): Promise<textmate.IToken[][]> {
  const grammar = await registry.loadGrammar('source.typst');
  expect(grammar).not.toBeNull();

  let stack = textmate.INITIAL;
  const lines: textmate.IToken[][] = [];
  for (const line of source.split('\n')) {
    const result = grammar!.tokenizeLine(line, stack);
    stack = result.ruleStack;
    lines.push(result.tokens);
  }
  return lines;
}

/** Scopes on the token covering `needle`, wherever in `source` it first is. */
async function scopesAt(
  source: string,
  needle: string,
  registry?: textmate.Registry,
): Promise<string[]> {
  const lines = source.split('\n');
  const row = lines.findIndex(line => line.includes(needle));
  expect(row, `${JSON.stringify(needle)} is not in the source`).toBeGreaterThanOrEqual(0);

  const column = lines[row].indexOf(needle);
  const tokens = (await tokenize(source, registry))[row];
  const token = tokens.find(t => t.startIndex <= column && column < t.endIndex);
  expect(token, `no token at ${row}:${column}`).toBeDefined();
  return token!.scopes;
}

const fence = (tag: string, ...body: string[]) => [`\`\`\`${tag}`, ...body, '```', ''].join('\n');

describe('a raw block with a language tag', () => {
  it('hands its body to that language', async () => {
    const scopes = await scopesAt(fence('rust', 'fn main() {}'), 'fn main');
    expect(scopes).toContain('meta.embedded.block.rust');
    expect(scopes).toContain('meta.word.source.rust');
  });

  it('accepts every alias, in any case', async () => {
    for (const tag of ['rust', 'rs', 'Rust', 'RS']) {
      const scopes = await scopesAt(fence(tag, 'fn main() {}'), 'fn main');
      expect(scopes, tag).toContain('meta.embedded.block.rust');
    }
  });

  it('picks the longest tag, so ```c++ is not ```c', async () => {
    expect(await scopesAt(fence('c++', 'int main() {}'), 'int main')).toContain(
      'meta.embedded.block.cpp',
    );
    expect(await scopesAt(fence('c', 'int main() {}'), 'int main')).toContain(
      'meta.embedded.block.c',
    );
    expect(await scopesAt(fence('objc++', 'int main() {}'), 'int main')).toContain(
      'meta.embedded.block.objective-cpp',
    );
  });

  it('closes on the fence that opened it, and not before', async () => {
    // Four backticks, so the three inside are content — which is the whole
    // point of matching the closing run by back-reference.
    const source = ['````python', 'print("```")', 'x = 1', '````', 'After the block.'].join('\n');

    expect(await scopesAt(source, 'print')).toContain('meta.embedded.block.python');
    expect(await scopesAt(source, 'x = 1')).toContain('meta.embedded.block.python');
    expect(await scopesAt(source, 'After')).not.toContain('meta.embedded.block.python');
  });

  it('works inline, on one line', async () => {
    const scopes = await scopesAt('Try ```rs let x = 1``` today.\n', 'let x');
    expect(scopes).toContain('meta.embedded.block.rust');
    expect(await scopesAt('Try ```rs let x = 1``` today.\n', 'today')).not.toContain(
      'meta.embedded.block.rust',
    );
  });

  it('gives every language in the table a working fence', async () => {
    for (const language of LANGUAGES) {
      for (const tag of language.tags) {
        const scopes = await scopesAt(fence(tag, 'body'), 'body');
        expect(scopes, `${language.id} via ${tag}`).toContain(
          `meta.embedded.block.${language.id}`,
        );
      }
    }
  });
});

describe('a raw block without one', () => {
  it('stays raw when the tag is a language nothing here knows', async () => {
    const scopes = await scopesAt(fence('brainfuck', '+++.'), '+++.');
    expect(scopes).toContain('markup.raw.block.typst');
    expect(scopes.some(scope => scope.startsWith('meta.embedded'))).toBe(false);
  });

  it('stays raw when there is no tag at all', async () => {
    const scopes = await scopesAt(fence('', 'plain text'), 'plain');
    expect(scopes).toContain('markup.raw.block.typst');
    expect(scopes.some(scope => scope.startsWith('meta.embedded'))).toBe(false);
  });

  it('does not swallow the document after it', async () => {
    const source = [...fence('rust', 'fn main() {}').split('\n'), '= A heading', ''].join('\n');
    expect(await scopesAt(source, 'A heading')).toContain('markup.heading.typst');
  });

  it('leaves inline raw and the rest of the grammar alone', async () => {
    expect(await scopesAt('A `code` span.\n', '`code`')).toContain('markup.raw.inline.typst');
    expect(await scopesAt('= Heading\n', '= Heading')).toContain('markup.heading.typst');
  });
});

/**
 * Every bracket in `source`, paired the way VS Code's bracket-pair colouring
 * pairs them.
 *
 * That feature reads the *standard* token type — Other, Comment, String, RegEx
 * — which vscode-textmate derives from the scope name, and skips any bracket
 * that is not Other. Semantic tokens never reach it. So a string the grammar
 * fails to scope is a string whose brackets count, and whose `//` opens a
 * comment that swallows every bracket after it on the line.
 *
 * Returns one `depth` per bracket, in document order; a matched pair shares a
 * depth, and the colour is that depth modulo the palette.
 */
async function bracketDepths(source: string): Promise<{ bracket: string; depth: number }[]> {
  const grammar = await stubbed.loadGrammar('source.typst');
  const brackets: { bracket: string; depth: number }[] = [];

  let stack = textmate.INITIAL;
  let depth = 0;
  for (const line of source.split('\n')) {
    const { tokens, ruleStack } = grammar!.tokenizeLine2(line, stack);
    stack = ruleStack;

    for (let i = 0; i < tokens.length; i += 2) {
      const start = tokens[i];
      const end = i + 2 < tokens.length ? tokens[i + 2] : line.length;
      // Bits 8–9 of the metadata are the standard token type; 0 is Other.
      if (((tokens[i + 1] >>> 8) & 0b11) !== 0) continue;

      for (const bracket of line.slice(start, end)) {
        if ('([{'.includes(bracket)) brackets.push({ bracket, depth: depth++ });
        else if (')]}'.includes(bracket)) brackets.push({ bracket, depth: --depth });
      }
    }
  }
  return brackets;
}

describe('bracket pair colouring', () => {
  it('does not let a URL in a string open a comment', async () => {
    const scopes = await scopesAt('#link("https://github.com/ada")\n', '//github');
    expect(scopes).toContain('string.quoted.double.typst');
    expect(scopes.some(scope => scope.startsWith('comment'))).toBe(false);
  });

  it('closes a call whose argument is a URL at the depth it opened', async () => {
    const depths = await bracketDepths('#link("https://x.example/a")\n');
    expect(depths).toEqual([
      { bracket: '(', depth: 0 },
      { bracket: ')', depth: 0 },
    ]);
  });

  it('does not count brackets inside a string as nesting', async () => {
    const depths = await bracketDepths('#let phone = ("+1 (555) 013-2400")\n');
    expect(depths).toEqual([
      { bracket: '(', depth: 0 },
      { bracket: ')', depth: 0 },
    ]);
  });

  it('keeps a nested array of URL pairs balanced all the way out', async () => {
    const source = [
      '#let profile = (',
      '  contacts: (',
      '    (text: "github.com/ada", url: "https://github.com/ada"),',
      '    (text: "+1 (555) 013-2400", url: none),',
      '  ),',
      ')',
      '',
    ].join('\n');

    // Depth per bracket, in order: the outer pair, `contacts`, then one pair
    // per entry — each opener meeting its closer at the same depth.
    expect((await bracketDepths(source)).map(b => b.depth)).toEqual([
      0, 1, 2, 2, 2, 2, 1, 0,
    ]);
  });

  it('still treats a real line comment as a comment', async () => {
    expect(await scopesAt('// a note (unclosed\n', 'a note')).toContain(
      'comment.line.double-slash.typst',
    );
    expect(await bracketDepths('#f() // (\n')).toEqual([
      { bracket: '(', depth: 0 },
      { bracket: ')', depth: 0 },
    ]);
  });

  it('ignores an unpaired quote instead of running to the end of the file', async () => {
    // In markup `"` is ordinary text. A begin/end string rule would open here
    // and never close, taking the rest of the document with it.
    const source = ['He said "look, and never stopped.', '#f(1)', ''].join('\n');
    expect(await bracketDepths(source)).toEqual([
      { bracket: '(', depth: 0 },
      { bracket: ')', depth: 0 },
    ]);
  });
});

describe.skipIf(!BUILTINS)('against the VS Code on this machine', () => {
  it('names a scope VS Code really ships, for every language it ships', () => {
    for (const language of LANGUAGES.filter(l => l.provider === undefined)) {
      expect(BUILTINS!.grammars.has(language.scope), `${language.id} (${language.scope})`).toBe(
        true,
      );
      expect(BUILTINS!.languageIds.has(language.id), `language id ${language.id}`).toBe(true);
    }
  });

  it('marks as provided-by-an-extension only what VS Code does not ship', () => {
    for (const language of LANGUAGES.filter(l => l.provider === 'extension')) {
      expect(BUILTINS!.grammars.has(language.scope), `${language.id} is built in now`).toBe(false);
    }
  });

  it('claims only its own grammar as `self`', () => {
    const own = LANGUAGES.filter(l => l.provider === 'self');
    expect(own.map(l => l.scope)).toEqual(['source.typst']);
    expect(
      JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8')).name,
    ).toBe(EXTENSION_ID);
  });

  it('colours a rust block with rust, not with a stub', async () => {
    const scopes = await scopesAt(fence('rust', 'fn main() {}'), 'fn', real);
    expect(scopes).toContain('meta.embedded.block.rust');
    expect(scopes.some(scope => scope.endsWith('.rust') && scope !== 'meta.embedded.block.rust')).toBe(
      true,
    );
  });

  it('colours python, yaml and json blocks with their own grammars', async () => {
    expect(await scopesAt(fence('py', 'def f():'), 'def', real)).toContain(
      'storage.type.function.python',
    );
    expect(await scopesAt(fence('yaml', 'key: value'), 'key', real)).toContain(
      'entity.name.tag.yaml',
    );
    expect(await scopesAt(fence('json', '{"a": 1}'), '"a"', real)).toContain(
      'support.type.property-name.json',
    );
  });

  it('keeps the fence and the tag typst’s own', async () => {
    const source = fence('rust', 'fn main() {}');
    expect(await scopesAt(source, '```', real)).toContain('punctuation.definition.raw.typst');
    expect(await scopesAt(source, 'rust', real)).toContain('fenced_code.block.language.typst');
  });
});
