import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The embedded-language grammar, as data.
 *
 * `scripts/embedded/languages.json` is the table a maintainer edits;
 * `syntaxes/typst-embedded.tmLanguage.json` and the `embeddedLanguages` map in
 * `package.json` are expanded from it. Nothing at runtime reads any of the
 * three — VSCode does — so this suite is where a mistake in them surfaces.
 *
 * It lives in `src/` rather than beside the grammar because `.vscodeignore`
 * keeps `src/` out of the VSIX, and `**​/*.test.ts` with it.
 */

const ROOT = path.join(__dirname, '..');
const GENERATOR = path.join(ROOT, 'scripts', 'embedded', 'generate.mjs');

interface EmbeddedLanguage {
  id: string;
  scope: string;
  tags: string[];
  provider?: 'extension' | 'self';
}

interface Rule {
  name: string;
  contentName: string;
  begin: string;
  end: string;
  patterns: { include: string }[];
}

interface Grammar {
  scopeName: string;
  patterns: { include: string }[];
  repository: Record<string, Rule>;
}

function read<T>(...segments: string[]): T {
  return JSON.parse(fs.readFileSync(path.join(ROOT, ...segments), 'utf8')) as T;
}

const LANGUAGES = read<{ languages: EmbeddedLanguage[] }>(
  'scripts',
  'embedded',
  'languages.json',
).languages;

const EMBEDDED = read<Grammar>('syntaxes', 'typst-embedded.tmLanguage.json');
const TYPST = read<Grammar>('syntaxes', 'typst.tmLanguage.json');

const MANIFEST = read<{
  scripts: Record<string, string>;
  contributes: {
    grammars: {
      language?: string;
      scopeName: string;
      path: string;
      embeddedLanguages?: Record<string, string>;
    }[];
  };
}>('package.json');

const CONTRIBUTED = MANIFEST.contributes.grammars;

/**
 * A `begin` pattern as something JavaScript can run.
 *
 * Oniguruma's inline-option group `(?i:…)` has no JS equivalent, and it is the
 * only construct the generator emits that JS does not understand — so it
 * becomes a plain group and the flag moves onto the expression.
 */
function asJsRegExp(pattern: string): RegExp {
  return new RegExp(pattern.replace(/\(\?i:/g, '(?:'), 'i');
}

describe('the embedded-language table', () => {
  it('names each language once', () => {
    const ids = LANGUAGES.map(language => language.id);
    expect(new Set(ids).size, ids.join(' ')).toBe(ids.length);
  });

  it('lets exactly one language claim each tag', () => {
    const owner = new Map<string, string>();
    for (const language of LANGUAGES) {
      for (const tag of language.tags) {
        expect(owner.get(tag) ?? language.id, `tag "${tag}"`).toBe(language.id);
        owner.set(tag, language.id);
      }
    }
  });

  it('uses tags a user could type after the backticks', () => {
    for (const language of LANGUAGES) {
      expect(language.tags.length, language.id).toBeGreaterThan(0);
      for (const tag of language.tags) {
        expect(tag, language.id).toBe(tag.toLowerCase());
        expect(tag, language.id).toMatch(/^[a-z0-9][a-z0-9+#._-]*$/);
      }
    }
  });

  it('points every language at a plausible TextMate scope', () => {
    for (const language of LANGUAGES) {
      expect(language.scope, language.id).toMatch(/^(source|text)\.[a-z0-9.+-]+$/);
    }
  });

  it('is sorted, so a new entry has one obvious home', () => {
    const ids = LANGUAGES.map(language => language.id);
    expect(ids).toEqual([...ids].sort());
  });
});

describe('the generated grammar', () => {
  it('is what the table expands to today', () => {
    // The generator's own `--check`: no reimplementation of it here, so this
    // fails on a forgotten `pnpm run build:grammar` and on nothing else.
    execFileSync('node', [GENERATOR, '--check'], { stdio: 'pipe' });
  });

  it('is contributed under the scope it declares', () => {
    const contribution = CONTRIBUTED.find(g => g.scopeName === EMBEDDED.scopeName);
    expect(contribution?.path).toBe('./syntaxes/typst-embedded.tmLanguage.json');
    expect(EMBEDDED.scopeName).toBe('source.typst.embedded');
    expect(MANIFEST.scripts['build:grammar']).toBeDefined();
  });

  it('gives every language a rule that defers to its grammar', () => {
    expect(Object.keys(EMBEDDED.repository)).toEqual(LANGUAGES.map(l => l.id));
    expect(EMBEDDED.patterns).toEqual(LANGUAGES.map(l => ({ include: `#${l.id}` })));

    for (const language of LANGUAGES) {
      const rule = EMBEDDED.repository[language.id];
      expect(rule.patterns, language.id).toEqual([{ include: language.scope }]);
      expect(rule.contentName, language.id).toBe(`meta.embedded.block.${language.id}`);
      // The closing fence has to be the one that opened the block, or a
      // four-backtick block could not contain three.
      expect(rule.end, language.id).toBe('(\\1)');
    }
  });

  it('tells VSCode which editor mode each block is in', () => {
    const typst = CONTRIBUTED.find(g => g.language === 'typst');
    expect(typst?.embeddedLanguages).toEqual(
      Object.fromEntries(
        LANGUAGES.map(l => [`meta.embedded.block.${l.id}`, l.id]),
      ),
    );
  });
});

describe('the fences a rule matches', () => {
  it('claims every one of its own tags, in any case', () => {
    for (const language of LANGUAGES) {
      const begin = asJsRegExp(EMBEDDED.repository[language.id].begin);
      for (const tag of language.tags) {
        for (const fence of [`\`\`\`${tag}\n`, `\`\`\`${tag.toUpperCase()}\n`, `\`\`\`\`${tag} x\n`]) {
          expect(begin.test(fence), `${language.id}: ${JSON.stringify(fence)}`).toBe(true);
        }
      }
    }
  });

  it('claims no tag another language owns', () => {
    for (const language of LANGUAGES) {
      const begin = asJsRegExp(EMBEDDED.repository[language.id].begin);
      for (const other of LANGUAGES) {
        if (other.id === language.id) continue;
        for (const tag of other.tags) {
          expect(begin.test(`\`\`\`${tag}\n`), `${language.id} took ${other.id}'s "${tag}"`).toBe(
            false,
          );
        }
      }
    }
  });

  // The reason the tag is followed by `(?=[\s`]|$)` rather than a word
  // boundary: `+` and `#` are not word characters, so `c` would otherwise
  // swallow the `c` of ```c++ and leave `++` as code.
  it('stops at the end of the tag, not in the middle of it', () => {
    const cpp = asJsRegExp(EMBEDDED.repository.cpp.begin);
    const c = asJsRegExp(EMBEDDED.repository.c.begin);
    expect(cpp.test('```c++\n')).toBe(true);
    expect(c.test('```c++\n')).toBe(false);
    expect(c.test('```c\n')).toBe(true);

    const rust = asJsRegExp(EMBEDDED.repository.rust.begin);
    expect(rust.test('```rustacean\n')).toBe(false);
    expect(rust.test('```rs let x = 1```\n')).toBe(true);
  });
});

describe('the typst grammar', () => {
  it('tries the embedded rules before its own raw block', () => {
    const includes = TYPST.patterns.map(pattern => pattern.include);
    expect(includes).toContain('source.typst.embedded');
    expect(includes.indexOf('source.typst.embedded')).toBeLessThan(
      includes.indexOf('#raw-block'),
    );
  });

  it('still colours a block whose tag nothing claims', () => {
    const raw = TYPST.repository['raw-block'] as unknown as Rule;
    expect(asJsRegExp(raw.begin).test('```whatever\n')).toBe(true);
    expect(asJsRegExp(raw.begin).test('```\n')).toBe(true);
    expect(raw.end).toBe('(\\1)');
  });
});
