import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ASKAMA_LANGUAGES } from './languages';

const root = path.resolve(__dirname, '..');

interface LanguageContribution {
  id: string;
  aliases: string[];
  extensions: string[];
  filenames?: string[];
  filenamePatterns?: string[];
  configuration: string;
}

/** Every way a language claims files, flattened for whole-set assertions. */
function associations(lang: LanguageContribution): string[] {
  return [...lang.extensions, ...(lang.filenames ?? []), ...(lang.filenamePatterns ?? [])];
}

interface GrammarContribution {
  language?: string;
  scopeName: string;
  path: string;
  injectTo?: string[];
}

const pkg = JSON.parse(readFileSync(`${root}/package.json`, 'utf8')) as {
  contributes: {
    languages: LanguageContribution[];
    grammars: GrammarContribution[];
    snippets: { language: string; path: string }[];
  };
};

const { languages, grammars, snippets } = pkg.contributes;
const languageGrammars = grammars.filter(g => g.language !== undefined);

describe('language contributions', () => {
  it('declares a grammar and snippets for every language', () => {
    const ids = languages.map(l => l.id);
    expect(languageGrammars.map(g => g.language).sort()).toEqual([...ids].sort());
    expect(snippets.map(s => s.language).sort()).toEqual([...ids].sort());
  });

  it('matches the language set used for decorations', () => {
    expect([...ASKAMA_LANGUAGES].sort()).toEqual(languages.map(l => l.id).sort());
  });

  it('never lets two languages claim the same file association', () => {
    const seen = new Map<string, string>();
    for (const lang of languages) {
      for (const assoc of associations(lang)) {
        const owner = seen.get(assoc) ?? lang.id;
        expect(owner, `${assoc} claimed by ${owner} and ${lang.id}`).toBe(lang.id);
        seen.set(assoc, lang.id);
      }
    }
  });

  // VS Code resolves a file to a language by exact filename first, then by the
  // longest matching filenamePattern, then by the longest matching extension.
  // The built-in dotenv language claims the pattern ".env.*", which outranks our
  // ".env.askama" extension — so the literal dotfile names have to be filenames.
  it('claims literal dotfile names by exact filename', () => {
    const env = languages.find(l => l.id === 'askama-env');
    for (const name of ['.env.askama', '.env.j2', '.env.jinja', '.env.jinja2']) {
      expect(env?.filenames, name).toContain(name);
    }
  });

  it('covers every supported host language', () => {
    expect(languages.map(l => l.id).sort()).toEqual([
      'askama-capnp',
      'askama-css',
      'askama-env',
      'askama-gitignore',
      'askama-html',
      'askama-js',
      'askama-json',
      'askama-jsx',
      'askama-md',
      'askama-rust',
      'askama-swift',
      'askama-toml',
      'askama-ts',
      'askama-tsx',
      'askama-txt',
      'askama-yaml',
    ]);
  });

  it('declares all four template suffixes for every base name', () => {
    const suffixes = ['.askama', '.j2', '.jinja', '.jinja2'];
    for (const lang of languages) {
      const bases = new Map<string, Set<string>>();
      for (const assoc of associations(lang)) {
        const suffix = suffixes.find(s => assoc.endsWith(s));
        expect(suffix, `${assoc} (${lang.id}) has no template suffix`).toBeDefined();
        const base = assoc.slice(0, assoc.length - suffix!.length);
        bases.set(base, (bases.get(base) ?? new Set()).add(suffix!));
      }
      for (const [base, found] of bases) {
        expect([...found].sort(), `${lang.id} base "${base}"`).toEqual([...suffixes].sort());
      }
    }
  });
});

describe('grammar contributions', () => {
  it('points at grammar files whose scopeName matches the contribution', () => {
    for (const grammar of grammars) {
      const source = JSON.parse(readFileSync(`${root}/${grammar.path}`, 'utf8')) as { scopeName: string };
      expect(source.scopeName, grammar.path).toBe(grammar.scopeName);
    }
  });

  it('injects askama patterns into every language scope', () => {
    const injection = grammars.find(g => g.scopeName === 'askama.injection');
    expect(injection).toBeDefined();
    const scopes = languageGrammars.map(g => g.scopeName).filter(s => s !== 'text.askama');
    expect([...injection!.injectTo!].sort()).toEqual([...scopes].sort());

    const source = JSON.parse(readFileSync(`${root}/${injection!.path}`, 'utf8')) as {
      injectionSelector: string;
    };
    for (const scope of injection!.injectTo!) {
      expect(source.injectionSelector, scope).toContain(`L:${scope} `);
    }
  });
});
