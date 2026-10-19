import * as fs from 'node:fs';
import * as path from 'node:path';
import { beforeAll, describe, expect, it } from 'vitest';
import * as oniguruma from 'vscode-oniguruma';
import * as textmate from 'vscode-textmate';

/**
 * The generated language grammars, tokenized against the host grammars VS Code
 * really ships — so a `host` scope that exists but swallows `{{` (a string rule
 * that never lets the injection in, say) shows up here, not in the editor.
 *
 * Skipped on a machine without VS Code installed; `grammar.test.ts` covers the
 * one language whose grammar is self-contained.
 */

const EXT_ROOT = path.resolve(__dirname, '..');

interface TemplateLanguage {
  id: string;
  scope: string;
  host?: string;
  provider?: 'extension';
}

const LANGUAGES = (
  JSON.parse(
    fs.readFileSync(path.join(EXT_ROOT, 'scripts', 'languages', 'languages.json'), 'utf8'),
  ) as { languages: TemplateLanguage[] }
).languages;

/** The `extensions` directory of a VS Code install, if this machine has one. */
const APP_EXTENSIONS = [
  '/Applications/Visual Studio Code.app/Contents/Resources/app/extensions',
  '/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/extensions',
  '/Applications/Cursor.app/Contents/Resources/app/extensions',
  '/usr/share/code/resources/app/extensions',
].find(candidate => fs.existsSync(candidate));

/** TextMate scope → grammar file, for VS Code's built-ins and this extension. */
function grammarFiles(): Map<string, string> {
  const files = new Map<string, string>();
  for (const name of fs.readdirSync(APP_EXTENSIONS!)) {
    const manifest = path.join(APP_EXTENSIONS!, name, 'package.json');
    if (!fs.existsSync(manifest)) continue;
    try {
      for (const grammar of JSON.parse(fs.readFileSync(manifest, 'utf8')).contributes?.grammars ?? []) {
        files.set(grammar.scopeName, path.join(APP_EXTENSIONS!, name, grammar.path));
      }
    } catch {
      continue;
    }
  }
  const own = JSON.parse(fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8'));
  for (const grammar of own.contributes.grammars) {
    files.set(grammar.scopeName, path.join(EXT_ROOT, grammar.path));
  }
  return files;
}

let registry: textmate.Registry;

beforeAll(async () => {
  if (!APP_EXTENSIONS) return;
  const wasm = path.join(EXT_ROOT, 'node_modules', 'vscode-oniguruma', 'release', 'onig.wasm');
  await oniguruma.loadWASM(fs.readFileSync(wasm).buffer as ArrayBuffer);

  const files = grammarFiles();
  const injected = new Set(LANGUAGES.filter(l => l.host).map(l => l.scope));
  registry = new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: sources => new oniguruma.OnigScanner(sources),
      createOnigString: str => new oniguruma.OnigString(str),
    }),
    loadGrammar: async scopeName => {
      const file = files.get(scopeName);
      if (!file) return null;
      return textmate.parseRawGrammar(fs.readFileSync(file, 'utf8'), file);
    },
    getInjections: scopeName => (injected.has(scopeName) ? ['askama.injection'] : undefined),
  });
});

/** Scopes of the token covering `needle`, which must appear on exactly one line. */
async function scopesOf(scope: string, source: string, needle: string): Promise<string[]> {
  const grammar = await registry.loadGrammar(scope);
  expect(grammar, scope).not.toBeNull();
  const lines = source.split('\n');
  const lineIndex = lines.findIndex(l => l.includes(needle));
  expect(lineIndex, `"${needle}" not found in source`).toBeGreaterThanOrEqual(0);

  let ruleStack = textmate.INITIAL;
  let tokens: textmate.IToken[] = [];
  for (let i = 0; i <= lineIndex; i++) {
    const result = grammar!.tokenizeLine(lines[i], ruleStack);
    ruleStack = result.ruleStack;
    tokens = result.tokens;
  }
  const column = lines[lineIndex].indexOf(needle);
  const token = tokens.find(t => t.startIndex <= column && column < t.endIndex);
  expect(token, `no token at ${lineIndex}:${column}`).toBeDefined();
  return token!.scopes;
}

describe.skipIf(!APP_EXTENSIONS)('every language with a built-in host', () => {
  const hosted = LANGUAGES.filter(l => l.host && !l.provider);

  it('scopes Askama statements and expressions between host lines', async () => {
    const source = ['{% for item in items %}', '{{ item.name }}', '{% endfor %}'].join('\n');
    for (const language of hosted) {
      expect(await scopesOf(language.scope, source, 'for'), language.id).toContain(
        'keyword.control.askama',
      );
      expect(await scopesOf(language.scope, source, '{{ item'), language.id).toContain(
        'meta.expression.askama',
      );
    }
  });
});

describe.skipIf(!APP_EXTENSIONS)('tags inside host constructs', () => {
  // One case per language: a tag where the host grammar has opened something of
  // its own, which only the injection reaches.
  const cases: [id: string, source: string, needle: string, hostScope: string][] = [
    ['askama-sh', 'echo "Hello, {{ name }}"', '{{ name }}', 'string.quoted.double.shell'],
    ['askama-objc', 'NSString *s = @"{{ name }}";', '{{ name }}', 'string.quoted.double.objc'],
    ['askama-xml', '<string id="{{ id }}">x</string>', '{{ id }}', 'string.quoted.double.xml'],
    ['askama-gradle', "version = '{{ version }}'", '{{ version }}', 'string.quoted.single.groovy'],
  ];

  for (const [id, source, needle, hostScope] of cases) {
    it(`reaches into ${hostScope} in ${id}`, async () => {
      const scope = LANGUAGES.find(l => l.id === id)!.scope;
      const scopes = await scopesOf(scope, source, needle);
      expect(scopes).toContain('meta.expression.askama');
      expect(scopes).toContain(hostScope);
    });
  }

  it('lets a plist keep its XML structure around the tags', async () => {
    const scope = LANGUAGES.find(l => l.id === 'askama-xml')!.scope;
    const source = [
      '<?xml version="1.0" encoding="UTF-8"?>',
      '<plist version="1.0">',
      '<dict>',
      '  <key>CFBundleIdentifier</key>',
      '  <string>{{ bundle_id }}</string>',
      '{% if debug %}',
      '  <key>Debug</key><true/>',
      '{% endif %}',
      '</dict>',
      '</plist>',
    ].join('\n');
    expect(await scopesOf(scope, source, 'dict>')).toContain('entity.name.tag.localname.xml');
    expect(await scopesOf(scope, source, '{{ bundle_id }}')).toContain('meta.expression.askama');
    expect(await scopesOf(scope, source, 'if debug')).toContain('meta.statement.askama');
  });
});
