import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ASKAMA_LANGUAGES } from './languages';

const root = path.resolve(__dirname, '..');

interface LanguageContribution {
  id: string;
  aliases: string[];
  extensions: string[];
  configuration: string;
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

  it('gives every language a unique, non-overlapping set of file extensions', () => {
    const seen = new Map<string, string>();
    for (const lang of languages) {
      for (const ext of lang.extensions) {
        expect(seen.get(ext), `${ext} claimed by ${seen.get(ext)} and ${lang.id}`).toBeUndefined();
        seen.set(ext, lang.id);
      }
    }
  });

  it('includes askama markdown', () => {
    const md = languages.find(l => l.id === 'askama-md');
    expect(md?.extensions).toContain('.md.askama');
    expect(md?.extensions).toContain('.md.jinja2');
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
