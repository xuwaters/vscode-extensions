import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ASKAMA_LANGUAGES } from './languages';

const root = path.resolve(__dirname, '..');
const GENERATOR = path.join(root, 'scripts', 'languages', 'generate.mjs');

interface TemplateLanguage {
  id: string;
  host?: string;
  provider?: 'extension';
}

const TABLE = JSON.parse(
  readFileSync(path.join(root, 'scripts', 'languages', 'languages.json'), 'utf8'),
) as { languages: TemplateLanguage[] };

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
      'askama-gradle',
      'askama-html',
      'askama-js',
      'askama-json',
      'askama-jsx',
      'askama-md',
      'askama-objc',
      'askama-rust',
      'askama-sh',
      'askama-swift',
      'askama-toml',
      'askama-ts',
      'askama-tsx',
      'askama-txt',
      'askama-xml',
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

describe('the language table', () => {
  it('is what package.json, the grammars and languages.ts are generated from', () => {
    // The generator's own `--check`, so a forgotten regeneration fails here.
    execFileSync('node', [GENERATOR, '--check'], { stdio: 'pipe' });
  });

  it('names each language once', () => {
    const ids = TABLE.languages.map(l => l.id);
    expect(new Set(ids).size, ids.join(' ')).toBe(ids.length);
  });
});

/** The `extensions` directory of a VS Code install, if this machine has one. */
const APP_EXTENSIONS = [
  '/Applications/Visual Studio Code.app/Contents/Resources/app/extensions',
  '/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/extensions',
  '/Applications/Cursor.app/Contents/Resources/app/extensions',
  '/usr/share/code/resources/app/extensions',
].find(candidate => existsSync(candidate));

/** Every TextMate scope the VS Code at `directory` ships a grammar for. */
function builtinScopes(directory: string): Set<string> {
  const scopes = new Set<string>();
  for (const name of readdirSync(directory)) {
    const manifest = path.join(directory, name, 'package.json');
    if (!existsSync(manifest)) continue;
    try {
      const contributes = JSON.parse(readFileSync(manifest, 'utf8')).contributes;
      for (const grammar of contributes?.grammars ?? []) scopes.add(grammar.scopeName);
    } catch {
      continue;
    }
  }
  return scopes;
}

describe.skipIf(!APP_EXTENSIONS)('against the VS Code on this machine', () => {
  const scopes = APP_EXTENSIONS ? builtinScopes(APP_EXTENSIONS) : new Set<string>();

  it('includes a host grammar VS Code really ships, for every host it ships', () => {
    for (const language of TABLE.languages.filter(l => l.host && !l.provider)) {
      expect(scopes.has(language.host!), `${language.id} (${language.host})`).toBe(true);
    }
  });

  it('marks as provided-by-an-extension only what VS Code does not ship', () => {
    for (const language of TABLE.languages.filter(l => l.provider === 'extension')) {
      expect(scopes.has(language.host!), `${language.id} is built in now`).toBe(false);
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
