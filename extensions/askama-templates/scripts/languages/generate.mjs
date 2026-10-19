// Expands `languages.json` into every place VS Code, and the extension, read a
// template language from:
//
//   * `package.json` → `contributes.languages`, `contributes.grammars` and
//     `contributes.snippets` — the file associations, one grammar per language
//     plus the injection's `injectTo`, and the shared snippet file.
//   * `syntaxes/askama-<name>.tmLanguage.json` — the language grammar: Askama
//     tags first, then a `{% raw %}` rule, then the host grammar.
//   * `syntaxes/askama-injection.tmLanguage.json` → `injectionSelector`, which
//     has to name every scope `injectTo` does.
//   * `src/languages.ts` — the ids the decoration layer paints.
//
// A language touches all four, and they have to agree on every id and scope;
// adding one by hand meant editing five files in lockstep. Run it after editing
// the table:
//
//     pnpm --filter wx-vsce-askama-templates run build:languages
//
// `--check` writes nothing and exits non-zero if anything is stale, which is
// what `src/contributions.test.ts` runs — so a forgotten regeneration fails the
// suite rather than shipping.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const EXTENSION = join(HERE, '..', '..');

const TABLE = join(HERE, 'languages.json');
const MANIFEST = join(EXTENSION, 'package.json');
const INJECTION = join(EXTENSION, 'syntaxes', 'askama-injection.tmLanguage.json');
const LANGUAGES_TS = join(EXTENSION, 'src', 'languages.ts');

const SCHEMA = 'https://raw.githubusercontent.com/martinring/tmlanguage/master/tmlanguage.json';
const CONFIGURATION = './language-configuration.json';
const SNIPPETS = './snippets/askama.json';

/** What the injection must stay out of: Askama's own regions, and raw blocks. */
const INJECTION_EXCLUDES =
  '-comment.block.askama -meta.raw.block.askama -meta.statement.askama -meta.expression.askama';

/**
 * @typedef {object} TemplateLanguage
 * @property {string} id VS Code language id, `askama-<name>`.
 * @property {string} name Display name, after `Askama `.
 * @property {string} scope Scope the language's grammar registers.
 * @property {string} [host] Scope of the templated format's grammar.
 * @property {boolean} [bundled] The grammar file is hand-written; leave it.
 * @property {'extension'} [provider] The host grammar ships in a third-party
 *   extension, not in VS Code. Documentation only; nothing generated changes.
 * @property {string[]} extensions Base extensions, before the suffix.
 * @property {string[]} [filenames] Base file names, before the suffix.
 * @property {string[]} [filenamePatterns] Base globs, before the suffix.
 */

/** @returns {{ suffixes: string[], languages: TemplateLanguage[] }} */
export function readTable() {
  return JSON.parse(readFileSync(TABLE, 'utf8'));
}

/** @param {TemplateLanguage} language @returns {string} `askama-html` → `html`. */
function shortName(language) {
  return language.id.replace(/^askama-/, '');
}

/** @param {TemplateLanguage} language @returns {string} Manifest-relative path. */
function grammarPath(language) {
  return `./syntaxes/${language.id}.tmLanguage.json`;
}

/**
 * A language with neither a host nor a bundled grammar is plain text, which
 * `source.askama` already covers end to end — there is nothing to inject into.
 *
 * @param {TemplateLanguage} language
 * @returns {boolean}
 */
function isInjected(language) {
  return language.host !== undefined || language.bundled === true;
}

/**
 * Every base crossed with every suffix, base-major — `.md.askama`, `.md.j2`, …,
 * `.markdown.askama`, … — so each base's four names read together.
 *
 * @param {string[] | undefined} bases
 * @param {string[]} suffixes
 * @returns {string[] | undefined}
 */
function expand(bases, suffixes) {
  return bases?.flatMap((base) => suffixes.map((suffix) => base + suffix));
}

/**
 * @param {TemplateLanguage} language
 * @param {string[]} suffixes
 * @returns {object} The `contributes.languages` entry.
 */
function languageContribution(language, suffixes) {
  /** @type {Record<string, unknown>} */
  const entry = {
    id: language.id,
    aliases: [`Askama ${language.name}`, language.id],
    extensions: expand(language.extensions, suffixes),
  };
  if (language.filenames) entry.filenames = expand(language.filenames, suffixes);
  if (language.filenamePatterns) entry.filenamePatterns = expand(language.filenamePatterns, suffixes);
  entry.configuration = CONFIGURATION;
  return entry;
}

/** @param {string} include @returns {object} `{% raw %}…{% endraw %}`, coloured by `include` alone. */
function rawBlock(include) {
  const delimiters = {
    1: { name: 'punctuation.section.embedded.begin.askama' },
    2: { name: 'keyword.control.askama' },
    3: { name: 'punctuation.section.embedded.end.askama' },
  };
  return {
    name: 'meta.raw.block.askama',
    begin: '(\\{%[-+~]?)\\s*(raw)\\s*([-+~]?%\\})',
    end: '(\\{%[-+~]?)\\s*(endraw)\\s*([-+~]?%\\})',
    beginCaptures: delimiters,
    endCaptures: delimiters,
    patterns: [{ include }],
  };
}

/**
 * The language grammar, for a language with a host.
 *
 * Askama comments come first so `{# {{ x }} #}` stays a comment; the raw block
 * comes before statements so `{% raw %}` opens a block instead of matching as a
 * lone tag; the host grammar comes last, colouring whatever none of them claim.
 * The injection then re-applies the tags inside host constructs — a tag in an
 * HTML attribute string, say — which this top-level list cannot reach.
 *
 * @param {TemplateLanguage} language
 * @returns {object}
 */
export function buildGrammar(language) {
  if (language.host === undefined) {
    return {
      $schema: SCHEMA,
      name: `Askama ${language.name}`,
      scopeName: language.scope,
      patterns: [{ include: 'source.askama' }],
    };
  }
  const raw = `raw-block-${shortName(language)}`;
  return {
    $schema: SCHEMA,
    name: `Askama ${language.name}`,
    scopeName: language.scope,
    patterns: [
      { include: 'source.askama#comment' },
      { include: `#${raw}` },
      { include: 'source.askama#statement' },
      { include: 'source.askama#expression' },
      { include: language.host },
    ],
    repository: { [raw]: rawBlock(language.host) },
  };
}

/**
 * JSON in the grammars' hand-written style: an object holding only scalars sits
 * on one line (`{ "include": "source.rust" }`), everything else is expanded.
 *
 * @param {unknown} value
 * @param {string} [indent]
 * @returns {string}
 */
export function format(value, indent = '') {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  const inner = indent + '  ';
  if (Array.isArray(value)) {
    if (value.length === 0) return '[]';
    return `[\n${value.map((item) => inner + format(item, inner)).join(',\n')}\n${indent}]`;
  }
  const entries = Object.entries(value);
  if (entries.length === 0) return '{}';
  if (entries.every(([, v]) => v === null || typeof v !== 'object')) {
    return `{ ${entries.map(([k, v]) => `${JSON.stringify(k)}: ${JSON.stringify(v)}`).join(', ')} }`;
  }
  const lines = entries.map(([k, v]) => `${inner}${JSON.stringify(k)}: ${format(v, inner)}`);
  return `{\n${lines.join(',\n')}\n${indent}}`;
}

/** @param {TemplateLanguage[]} languages @returns {string} */
function buildLanguagesTs(languages) {
  const ids = languages.map((language) => `  '${language.id}',\n`).join('');
  return (
    '// Generated by scripts/languages/generate.mjs from scripts/languages/languages.json — do not edit.\n' +
    `export const ASKAMA_LANGUAGES = new Set([\n${ids}]);\n`
  );
}

/**
 * @param {string} path
 * @param {string} contents
 * @param {boolean} check
 * @returns {boolean} Whether the file was, or would be, changed.
 */
function put(path, contents, check) {
  let current;
  try {
    current = readFileSync(path, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === contents) return false;
  if (!check) writeFileSync(path, contents);
  return true;
}

/**
 * @param {{ check?: boolean, write?: (line: string) => void }} [options]
 * @returns {number} Process exit code.
 */
export function generate({ check = false, write = console.log } = {}) {
  const { suffixes, languages } = readTable();
  const injected = languages.filter(isInjected).map((language) => language.scope);

  const manifest = JSON.parse(readFileSync(MANIFEST, 'utf8'));
  const contributes = manifest.contributes;

  // The grammars no language owns — `source.askama` and the injection — keep
  // their place on either side of the per-language ones.
  /** @type {{ language?: string, scopeName: string, injectTo?: string[] }[]} */
  const unowned = contributes.grammars.filter(
    (/** @type {{ language?: string }} */ grammar) => grammar.language === undefined,
  );
  const base = unowned.find((grammar) => grammar.scopeName === 'source.askama');
  const injection = unowned.find((grammar) => grammar.scopeName === 'askama.injection');
  if (!base || !injection) throw new Error('package.json lost its source.askama or askama.injection grammar');
  injection.injectTo = injected;

  contributes.languages = languages.map((language) => languageContribution(language, suffixes));
  contributes.grammars = [
    base,
    ...languages.map((language) => ({
      language: language.id,
      scopeName: language.scope,
      path: grammarPath(language),
    })),
    injection,
  ];
  contributes.snippets = languages.map((language) => ({ language: language.id, path: SNIPPETS }));

  const injectionGrammar = JSON.parse(readFileSync(INJECTION, 'utf8'));
  injectionGrammar.injectionSelector = injected.map((scope) => `L:${scope} ${INJECTION_EXCLUDES}`).join(', ');

  const results = [
    put(MANIFEST, JSON.stringify(manifest, null, 2) + '\n', check),
    put(INJECTION, format(injectionGrammar) + '\n', check),
    put(LANGUAGES_TS, buildLanguagesTs(languages), check),
    ...languages
      .filter((language) => !language.bundled)
      .map((language) =>
        put(join(EXTENSION, grammarPath(language)), format(buildGrammar(language)) + '\n', check),
      ),
  ];
  const stale = results.filter(Boolean).length;

  if (!check) {
    write(`${languages.length} template languages, ${stale} file(s) updated`);
    return 0;
  }
  if (stale > 0) {
    write(`${stale} generated file(s) are stale. Run \`pnpm run build:languages\`.`);
    return 1;
  }
  return 0;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exitCode = generate({ check: process.argv.includes('--check') });
}
